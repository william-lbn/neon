#include "postgres.h"
#include "fmgr.h"

#include "executor/instrument.h"
#include "common/hashfn.h"
#include "lib/stringinfo.h"
#include "storage/bufmgr.h"
#include "storage/shmem.h"
#include "utils/builtins.h"
#include "utils/guc.h"
#include "utils/jsonb.h"
#include "utils/timestamp.h"

#include "hll.h"
#include "logical_buffer_demand.h"

PG_FUNCTION_INFO_V1(neon_logical_buffer_demand);

#ifdef PG_HAVE_SHARED_BUFFER_DEMAND_HOOK
typedef struct LogicalBufferDemand
{
	uint64		started_millis;
	ConcurrentSHLL hits;
	ConcurrentSHLL misses;
} LogicalBufferDemand;

static bool logical_buffer_demand_enabled;
static LogicalBufferDemand *logical_demand;
static shared_buffer_demand_hook_type previous_demand_hook;

static uint64
demand_monotonic_millis(void)
{
	instr_time	now;

	INSTR_TIME_SET_CURRENT(now);
	return INSTR_TIME_GET_NANOSEC(now) / UINT64CONST(1000000);
}

static void
observe_shared_buffer_demand(const RelFileLocator *locator, ForkNumber forknum,
							  BlockNumber blocknum, bool shared_hit)
{
	/* The locator includes relation incarnation and database/tablespace. */
	const uint32 key[] = {locator->spcOid, locator->dbOid, locator->relNumber,
						 (uint32) forknum, blocknum};
	uint32		hash;

	if (previous_demand_hook)
		previous_demand_hook(locator, forknum, blocknum, shared_hit);

	hash = hash_bytes((const unsigned char *) key, sizeof(key));
	addConcurrentSHLL(shared_hit ? &logical_demand->hits : &logical_demand->misses,
					  hash, demand_monotonic_millis() / 1000 + 1);
}
#endif

void
LogicalBufferDemandRegister(void)
{
#ifdef PG_HAVE_SHARED_BUFFER_DEMAND_HOOK
	DefineCustomBoolVariable("neon.logical_buffer_demand_enabled",
							 "Observe foreground demand in shared buffers.",
							 NULL, &logical_buffer_demand_enabled, false,
							 PGC_POSTMASTER, 0, NULL, NULL, NULL);
#endif
}

void
LogicalBufferDemandShmemRequest(void)
{
#ifdef PG_HAVE_SHARED_BUFFER_DEMAND_HOOK
	if (logical_buffer_demand_enabled)
		RequestAddinShmemSpace(MAXALIGN(sizeof(LogicalBufferDemand)));
#endif
}

void
LogicalBufferDemandShmemInit(void)
{
#ifdef PG_HAVE_SHARED_BUFFER_DEMAND_HOOK
	bool		found;

	if (!logical_buffer_demand_enabled)
		return;

	logical_demand = ShmemInitStruct("Neon logical buffer demand",
									 sizeof(LogicalBufferDemand), &found);
	if (!found)
	{
		logical_demand->started_millis = demand_monotonic_millis();
		initConcurrentSHLL(&logical_demand->hits);
		initConcurrentSHLL(&logical_demand->misses);
	}
	previous_demand_hook = shared_buffer_demand_hook;
	shared_buffer_demand_hook = observe_shared_buffer_demand;
#endif
}

Datum
neon_logical_buffer_demand(PG_FUNCTION_ARGS)
{
#ifdef PG_HAVE_SHARED_BUFFER_DEMAND_HOOK
	SHLLDemandEstimate estimates[SHLL_DEMAND_WINDOWS];
	uint64		cutoff;
	uint64		covered;
	StringInfoData result;

	if (logical_demand == NULL)
		PG_RETURN_NULL();

	cutoff = demand_monotonic_millis();
	if (cutoff < logical_demand->started_millis)
		PG_RETURN_NULL();

	covered = Min((cutoff - logical_demand->started_millis) / 1000,
				  SHLL_DEMAND_WINDOWS * UINT64CONST(60));

	estimateConcurrentSHLLDemand(&logical_demand->hits, &logical_demand->misses,
								 cutoff / 1000 + 1, estimates);
	initStringInfo(&result);
	appendStringInfo(&result,
					 "{\"version\":1,\"scope\":\"postmaster_shared_relation_demand\","
					 "\"block_bytes\":%d,\"covered_seconds\":" UINT64_FORMAT ",\"windows\":[",
					 BLCKSZ, covered);

	for (unsigned int i = 0; i < covered / 60; i++)
	{
		const SHLLDemandEstimate *estimate = &estimates[i];

		appendStringInfo(&result,
						 "%s{\"seconds\":%u,\"distinct_pages\":" UINT64_FORMAT
						 ",\"shared_hit_pages\":" UINT64_FORMAT
						 ",\"shared_miss_pages\":" UINT64_FORMAT "}",
						 i == 0 ? "" : ",", 60 * (i + 1),
						 estimate->combined, estimate->hits, estimate->misses);
	}
	appendStringInfoString(&result, "]}");
	return DirectFunctionCall1(jsonb_in, CStringGetDatum(result.data));
#else
	PG_RETURN_NULL();
#endif
}
