/* 仅测试模块：直接包含冻结的生产实现，不增加生产PG/Neon接口。 */
#include "postgres.h"
#include "fmgr.h"
#include "catalog/pg_class.h"
#include "utils/resowner.h"
#include "source/pgxn/neon_walredo/inmem_smgr.c"

PG_MODULE_MAGIC;
PG_FUNCTION_INFO_V1(pg18_redo_vectors);

Datum
pg18_redo_vectors(PG_FUNCTION_ARGS)
{
	SMgrRelationData reln = {0};
	int32 mode = PG_GETARG_INT32(0);
	char *a = palloc(BLCKSZ);
	char *b = palloc(BLCKSZ);
	char *x = palloc(BLCKSZ);
	char *y = palloc(BLCKSZ);
	const void *input[2] = {a, b};
	void *output[2] = {x, y};
	uint32 off = 0;
	PgAioReturn aio_result;
	PgAioWaitRef waitref;
	PgAioHandle *ioh;

	reln.smgr_rlocator.locator = (RelFileLocator) {1663, 1, 12345};
	reln.smgr_relpersistence = RELPERSISTENCE_PERMANENT;
	if (mode == 0)
		inmem_startreadv(NULL, &reln, MAIN_FORKNUM, 42, output, 0);
	if (mode == -1)
		(void) inmem_fd(&reln, MAIN_FORKNUM, 42, &off);
	if (mode == 4)
	{
		ioh = pgaio_io_acquire(CurrentResourceOwner, &aio_result);
		inmem_startreadv(ioh, &reln, MAIN_FORKNUM, 42, output, 0);
		elog(ERROR, "invalid batch unexpectedly returned");
	}
	if (mode != 2 && mode != 3)
		elog(ERROR, "unexpected test mode");
	memset(a, 0x5a, BLCKSZ);
	memset(b, 0xa5, BLCKSZ);
	inmem_init();
	inmem_writev(&reln, MAIN_FORKNUM, 42, input, 2, false);
	if (used_pages != 2 || locate_page(&reln, MAIN_FORKNUM, 42) ==
		locate_page(&reln, MAIN_FORKNUM, 43))
		elog(ERROR, "production vector write did not retain distinct blocks");
	inmem_readv(&reln, MAIN_FORKNUM, 42, output, 2);
	if (memcmp(a, x, BLCKSZ) || memcmp(b, y, BLCKSZ))
		elog(ERROR, "production vector read returned incorrect block offsets");
	if (mode == 3)
	{
		memset(x, 0, BLCKSZ);
		memset(y, 0, BLCKSZ);
		ioh = pgaio_io_acquire(CurrentResourceOwner, &aio_result);
		pgaio_io_get_wref(ioh, &waitref);
		inmem_startreadv(ioh, &reln, MAIN_FORKNUM, 42, output, 2);
		pgaio_wref_wait(&waitref);
		if (!pgaio_wref_check_done(&waitref) ||
			aio_result.result.status != PGAIO_RS_OK || aio_result.result.result != 2 ||
			memcmp(a, x, BLCKSZ) || memcmp(b, y, BLCKSZ))
			elog(ERROR, "production inmem AIO completion/result/reclaim differs");
		ioh = pgaio_io_acquire_nb(CurrentResourceOwner, &aio_result);
		if (ioh == NULL)
			elog(ERROR, "completed production inmem AIO handle was not reusable");
		pgaio_io_release(ioh);
	}
	inmem_init();
	inmem_readv(&reln, MAIN_FORKNUM, 42, output, 2);
	for (int i = 0; i < BLCKSZ; i++)
		if (x[i] || y[i])
			elog(ERROR, "production inmem reset did not discard pages");
	PG_RETURN_BOOL(true);
}
