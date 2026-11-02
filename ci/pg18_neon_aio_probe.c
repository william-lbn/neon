/* PG18 Neon远端AIO定向探针：只替单个测试relation注册md操作+真实Neon read回调。
 * 不注册生产权限，不构造AIO完成，不修改opaque handle，也不模拟Pageserver预算。 */
#include "postgres.h"
#include "fmgr.h"
#include <dlfcn.h>
#include "access/table.h"
#include "catalog/pg_class.h"
#include "miscadmin.h"
#include "storage/bufmgr.h"
#include "storage/md.h"
#include "storage/smgr.h"
#include "utils/builtins.h"
#include "utils/rel.h"

PG_MODULE_MAGIC;
PG_FUNCTION_INFO_V1(test_neon18_remote_setup);
PG_FUNCTION_INFO_V1(test_neon18_remote_counts);
PG_FUNCTION_INFO_V1(test_neon18_slru);

static RelFileLocator probe_locator;
static bool probe_ready;
static uint64 probe_calls;
static uint64 probe_errors;
static void (*real_startreadv) (PgAioHandle *, SMgrRelation, ForkNumber,
							   BlockNumber, void **, BlockNumber);

static void
admin(void)
{
	if (!superuser())
		ereport(ERROR, (errcode(ERRCODE_INSUFFICIENT_PRIVILEGE),
						errmsg("PG18 storage probe requires superuser")));
}

static bool
probe_owns(RelFileLocator rlocator, ProcNumber backend, char persistence)
{
	(void) persistence;
	return probe_ready && backend == INVALID_PROC_NUMBER &&
		RelFileLocatorEquals(rlocator, probe_locator);
}

static void
probe_startreadv(PgAioHandle *ioh, SMgrRelation reln, ForkNumber forknum,
				BlockNumber blocknum, void **buffers, BlockNumber nblocks)
{
	probe_calls++;
	PG_TRY();
	{
		real_startreadv(ioh, reln, forknum, blocknum, buffers, nblocks);
	}
	PG_CATCH();
	{
		probe_errors++;
		PG_RE_THROW();
	}
	PG_END_TRY();
}

Datum
test_neon18_remote_setup(PG_FUNCTION_ARGS)
{
	Relation rel;
	ForkNumber forknum = MAIN_FORKNUM;
	BlockNumber first = 0;
	f_smgr adapter = {
		.smgr_name = "neon-pg18-test-read-adapter",
		.smgr_open = mdopen,
		.smgr_close = mdclose,
		.smgr_create = mdcreate,
		.smgr_exists = mdexists,
		.smgr_unlink = mdunlink,
		.smgr_extend = mdextend,
		.smgr_zeroextend = mdzeroextend,
		.smgr_prefetch = mdprefetch,
		.smgr_maxcombine = mdmaxcombine,
		.smgr_readv = mdreadv,
		.smgr_startreadv = probe_startreadv,
		.smgr_writev = mdwritev,
		.smgr_writeback = mdwriteback,
		.smgr_nblocks = mdnblocks,
		.smgr_truncate = mdtruncate,
		.smgr_immedsync = mdimmedsync,
		.smgr_registersync = mdregistersync,
		.smgr_fd = mdfd,
		.smgr_owns = probe_owns
	};

	admin();
	if (probe_ready)
		ereport(ERROR, (errmsg("one original probe registration per backend")));
	real_startreadv = dlsym(RTLD_DEFAULT, "neon_startreadv");
	if (real_startreadv == NULL)
		ereport(ERROR, (errmsg("actual loaded Neon PG18 callbacks absent")));
	rel = table_open(PG_GETARG_OID(0), AccessExclusiveLock);
	probe_locator = rel->rd_locator;
	/* 固定测试表先实际刷盘/驱逐；下一读不能命中旧shared Buffer绕过远端。 */
	FlushRelationBuffers(rel);
	DropRelationBuffers(RelationGetSmgr(rel), &forknum, 1, &first);
	RelationCloseSmgr(rel);
	table_close(rel, NoLock);
	smgrdestroyall();
	probe_ready = true;
	(void) smgrregister(&adapter);
	PG_RETURN_VOID();
}

Datum
test_neon18_remote_counts(PG_FUNCTION_ARGS)
{
	char *out;

	admin();
	out = psprintf("{\"calls\":\"" UINT64_FORMAT "\",\"errors\":\"" UINT64_FORMAT "\"}",
				   probe_calls, probe_errors);
	PG_RETURN_TEXT_P(cstring_to_text(out));
}

Datum
test_neon18_slru(PG_FUNCTION_ARGS)
{
	bool (*download) (const char *, int64);

	admin();
	download = dlsym(RTLD_DEFAULT, "neon_download_slru_segment");
	if (download == NULL)
		ereport(ERROR, (errmsg("actual Neon SLRU v3 callback absent")));
	PG_RETURN_BOOL(download(text_to_cstring(PG_GETARG_TEXT_PP(0)), PG_GETARG_INT64(1)));
}
