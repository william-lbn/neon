/*-------------------------------------------------------------------------
 * neon_dsg_tests.c -- 在真实Neon扩展上验证缓存/协议/SLRU发布的测试桥。
 *
 * 不复制生产实现、不构造Buffer pin或物理回收事实。仅由独立临时测试集群
 * 显式CREATE FUNCTION调用；不加入已发布extension SQL，所有入口复核superuser。
 *-------------------------------------------------------------------------
 */
#include "postgres.h"

#include <sys/resource.h>

#include "access/relation.h"
#include "miscadmin.h"
#include "libpq/pqsignal.h"
#include "utils/builtins.h"
#include "utils/pg_lsn.h"
#include "utils/rel.h"
#if PG_VERSION_NUM >= 160000
#include "varatt.h"
#endif

#include "../neon/neon_lwlsncache.h"
#include "../neon/pagestore_client.h"
#include "../neon/slru_download.h"

PG_FUNCTION_INFO_V1(test_neon_slru_publish);
PG_FUNCTION_INFO_V1(test_neon_slru_unpack);
PG_FUNCTION_INFO_V1(test_neon_lwlsn);
PG_FUNCTION_INFO_V1(test_neon_slru_download);

typedef void (*NeonSlruPublishFn) (const char *, const void *, int);
typedef NeonResponse * (*NeonUnpackFn) (StringInfo);
typedef XLogRecPtr (*NeonLsnGetFn) (NRelFileInfo, ForkNumber, BlockNumber);
typedef XLogRecPtr (*NeonLsnSetFn) (XLogRecPtr, NRelFileInfo, ForkNumber, BlockNumber);
static void neon_test_require_superuser(void);

/*-
 * 调用真实独立下载hook：测试提供的是本地网络协议fixture，生产通信器仍实际
 * 建连/发送/解析并落盘；不替换page_server函数指针，不声称替代真实Pageserver端到端。
 */
Datum
test_neon_slru_download(PG_FUNCTION_ARGS)
{
	neon_test_require_superuser();
#if defined(NEON_SLRU_DOWNLOAD_API_VERSION) && NEON_SLRU_DOWNLOAD_API_VERSION == 2
	{
		typedef bool (*DownloadFn) (const char *, int);
		static DownloadFn download;

		AssertVariableIsOfType(&neon_download_slru_segment, DownloadFn);
		if (download == NULL)
			download = (DownloadFn) load_external_function("$libdir/neon", "neon_download_slru_segment", true, NULL);
		PG_RETURN_BOOL(download(text_to_cstring(PG_GETARG_TEXT_PP(0)), PG_GETARG_INT32(1)));
	}
#else
	ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("SLRU v2 download test requires the matching core API")));
	PG_RETURN_NULL();
#endif
}

static void
neon_test_require_superuser(void)
{
	if (!superuser())
		ereport(ERROR, (errcode(ERRCODE_INSUFFICIENT_PRIVILEGE),
						errmsg("Neon internal storage tests require superuser")));
}

/*-
 * 在真实文件系统上调用生产发布器。short_write只降低本Backend的RLIMIT_FSIZE，
 * 让实际pwrite产生短写；FINALLY恢复资源限额/信号处理，不能伪造返回值或成功回执。
 * 不允许将这个故障辅助函数用于已有数据库或生产Compute。
 */
Datum
test_neon_slru_publish(PG_FUNCTION_ARGS)
{
	static NeonSlruPublishFn publish;
	char	   *path;
	int			nblocks;
	int			fill;
	char	   *data;
	struct rlimit saved;
	pqsigfunc	saved_handler = NULL;
	bool		short_write;

	neon_test_require_superuser();
	path = text_to_cstring(PG_GETARG_TEXT_PP(0));
	nblocks = PG_GETARG_INT32(1);
	fill = PG_GETARG_INT32(2);
	short_write = PG_GETARG_BOOL(3);
	if (fill < 0 || fill > 255)
		ereport(ERROR, (errmsg("invalid Neon test fill byte")));
	AssertVariableIsOfType(&neon_publish_slru_segment, NeonSlruPublishFn);
	if (publish == NULL)
		publish = (NeonSlruPublishFn) load_external_function("$libdir/neon",
															 "neon_publish_slru_segment", true, NULL);
	data = palloc(BLCKSZ * SLRU_PAGES_PER_SEGMENT);
	memset(data, fill, BLCKSZ * SLRU_PAGES_PER_SEGMENT);
	if (short_write)
	{
		struct rlimit limit;

		if (getrlimit(RLIMIT_FSIZE, &saved) != 0)
			ereport(ERROR, (errmsg("could not read test file-size limit: %m")));
		limit = saved;
		limit.rlim_cur = Min(saved.rlim_cur, (rlim_t) 1024);
		if (setrlimit(RLIMIT_FSIZE, &limit) != 0)
			ereport(ERROR, (errmsg("could not set test file-size limit: %m")));
		saved_handler = pqsignal(SIGXFSZ, SIG_IGN);
	}
	PG_TRY();
	{
		publish(path, data, nblocks);
	}
	PG_FINALLY();
	{
		if (short_write)
		{
			if (setrlimit(RLIMIT_FSIZE, &saved) != 0)
				ereport(FATAL, (errmsg("could not restore test file-size limit: %m")));
			pqsignal(SIGXFSZ, saved_handler);
		}
	}
	PG_END_TRY();
	pfree(data);
	PG_RETURN_VOID();
}

/* 真实生产解析器接收完整二进制报文，覆盖乘法之前的长度边界和截断报文。 */
Datum
test_neon_slru_unpack(PG_FUNCTION_ARGS)
{
	static NeonUnpackFn unpack;
	bytea	   *payload;
	StringInfoData message;
	NeonResponse *response;
	int			nblocks;

	neon_test_require_superuser();
	payload = PG_GETARG_BYTEA_PP(0);
	AssertVariableIsOfType(&nm_unpack_response, NeonUnpackFn);
	if (unpack == NULL)
		unpack = (NeonUnpackFn) load_external_function("$libdir/neon", "nm_unpack_response", true, NULL);
	initStringInfo(&message);
	appendBinaryStringInfo(&message, VARDATA_ANY(payload), VARSIZE_ANY_EXHDR(payload));
	response = unpack(&message);
	if (response->tag != T_NeonGetSlruSegmentResponse)
		ereport(ERROR, (errmsg("Neon SLRU test expected a segment response")));
	nblocks = ((NeonGetSlruSegmentResponse *) response)->n_blocks;
	pfree(response);
	pfree(message.data);
	PG_RETURN_INT32(nblocks);
}

/*-
 * 使用真实relation身份和调用者提供的实际WAL LSN，验证生产共享hash/LRU锁。
 * NULL LSN只查询；非NULL使用生产setter更新同一缓存，不触及页面/事务状态。
 */
Datum
test_neon_lwlsn(PG_FUNCTION_ARGS)
{
	static NeonLsnGetFn get_lsn;
	static NeonLsnSetFn set_lsn;
	Relation	relation;
	NRelFileInfo info;
	int			block;
	XLogRecPtr	result;

	neon_test_require_superuser();
	if (PG_ARGISNULL(0) || PG_ARGISNULL(1))
		ereport(ERROR, (errmsg("Neon LSN test requires a relation and block")));
	block = PG_GETARG_INT32(1);
	if (block < 0)
		ereport(ERROR, (errmsg("Neon LSN test requires a nonnegative block")));
	AssertVariableIsOfType(&neon_get_lwlsn, NeonLsnGetFn);
	AssertVariableIsOfType(&neon_set_lwlsn_block, NeonLsnSetFn);
	if (get_lsn == NULL)
	{
		get_lsn = (NeonLsnGetFn) load_external_function("$libdir/neon", "neon_get_lwlsn", true, NULL);
		set_lsn = (NeonLsnSetFn) load_external_function("$libdir/neon", "neon_set_lwlsn_block", true, NULL);
	}
	relation = relation_open(PG_GETARG_OID(0), AccessShareLock);
	info = InfoFromRelation(relation);
	if (PG_ARGISNULL(2))
		result = get_lsn(info, MAIN_FORKNUM, block);
	else
		result = set_lsn(PG_GETARG_LSN(2), info, MAIN_FORKNUM, block);
	relation_close(relation, AccessShareLock);
	PG_RETURN_LSN(result);
}
