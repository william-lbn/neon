/*-------------------------------------------------------------------------
 *
 * hll.c
 *	  Sliding HyperLogLog cardinality estimator
 *
 * Portions Copyright (c) 2014-2023, PostgreSQL Global Development Group
 *
 * Implements https://hal.science/hal-00465313/document
 * 
 * Based on Hideaki Ohno's C++ implementation.  This is probably not ideally
 * suited to estimating the cardinality of very large sets;  in particular, we
 * have not attempted to further optimize the implementation as described in
 * the Heule, Nunkesser and Hall paper "HyperLogLog in Practice: Algorithmic
 * Engineering of a State of The Art Cardinality Estimation Algorithm".
 *
 * A sparse representation of HyperLogLog state is used, with fixed space
 * overhead.
 *
 * The copyright terms of Ohno's original version (the MIT license) follow.
 *
 * IDENTIFICATION
 *	  src/backend/lib/hyperloglog.c
 *
 *-------------------------------------------------------------------------
 */

/*
 * Copyright (c) 2013 Hideaki Ohno <hide.o.j55{at}gmail.com>
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the 'Software'), to
 * deal in the Software without restriction, including without limitation the
 * rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
 * sell copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED 'AS IS', WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
 * FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
 * IN THE SOFTWARE.
 */

#include <math.h>

#include "postgres.h"
#include "funcapi.h"
#include "port/pg_bitutils.h"
#include "utils/timestamp.h"
#include "hll.h"


#define POW_2_32			(4294967296.0)
#define NEG_POW_2_32		(-4294967296.0)

#define ALPHA_MM ((0.7213 / (1.0 + 1.079 / HLL_N_REGISTERS)) * HLL_N_REGISTERS * HLL_N_REGISTERS)

/*
 * Worker for addHyperLogLog().
 *
 * Calculates the position of the first set bit in first b bits of x argument
 * starting from the first, reading from most significant to least significant
 * bits.
 *
 * Example (when considering fist 10 bits of x):
 *
 * rho(x = 0b1000000000)   returns 1
 * rho(x = 0b0010000000)   returns 3
 * rho(x = 0b0000000000)   returns b + 1
 *
 * "The binary address determined by the first b bits of x"
 *
 * Return value "j" used to index bit pattern to watch.
 */
static inline uint8
rho(uint32 x, uint8 b)
{
	uint8		j = 1;

	if (x == 0)
		return b + 1;

	j = 32 - pg_leftmost_one_pos32(x);

	if (j > b)
		return b + 1;

	return j;
}

/*
 * Initialize HyperLogLog track state
 */
void
initSHLL(HyperLogLogState *cState)
{
	memset(cState->regs, 0, sizeof(cState->regs));
}

/*
 * Adds element to the estimator, from caller-supplied hash.
 *
 * It is critical that the hash value passed be an actual hash value, typically
 * generated using hash_any().  The algorithm relies on a specific bit-pattern
 * observable in conjunction with stochastic averaging.  There must be a
 * uniform distribution of bits in hash values for each distinct original value
 * observed.
 */
void
addSHLL(HyperLogLogState *cState, uint32 hash)
{
	uint8		count;
	uint32		index;

	TimestampTz	now = GetCurrentTimestamp();
	/* Use the first "k" (registerWidth) bits as a zero based index */
	index = hash >> HLL_C_BITS;

	/* Compute the rank of the remaining 32 - "k" (registerWidth) bits */
	count = rho(hash << HLL_BIT_WIDTH, HLL_C_BITS) - 1;
	Assert(count <= HLL_C_BITS);
	cState->regs[index][count] = now;
}

static uint8
getMaximum(const TimestampTz* reg, TimestampTz since)
{
	uint8 max = 0;

	for (size_t i = 0; i < HLL_C_BITS + 1; i++)
	{
		if (reg[i] >= since)
		{
			max = i + 1;
		}
	}

	return max;
}


static double
estimateCardinality(double sum, unsigned int zero_count)
{
	double		result;

	/* result set to "raw" HyperLogLog estimate (E in the HyperLogLog paper) */
	result = ALPHA_MM / sum;

	if (result <= (5.0 / 2.0) * HLL_N_REGISTERS)
	{
		/* Small range correction */
		if (zero_count != 0)
			result = HLL_N_REGISTERS * log((double) HLL_N_REGISTERS /
										   zero_count);
	}
	else if (result > (1.0 / 30.0) * POW_2_32)
	{
		/* Large range correction */
		result = NEG_POW_2_32 * log(1.0 - (result / POW_2_32));
	}

	return result;
}

/*
 * Estimates cardinality, based on elements added so far
 */
double
estimateSHLL(HyperLogLogState *cState, time_t duration)
{
	double		sum = 0.0;
	size_t		i;
	unsigned int zero_count = 0;
	/* 0 indicates uninitialized timestamp, so if we need to cover the whole range than starts with 1 */
	TimestampTz since = duration == (time_t)-1 ? 1 : GetCurrentTimestamp() - duration * USECS_PER_SEC;

	for (i = 0; i < HLL_N_REGISTERS; i++)
	{
		const uint8 rank = getMaximum(cState->regs[i], since);

		zero_count += rank == 0;
		sum += 1.0 / pow(2.0, rank);
	}

	return estimateCardinality(sum, zero_count);
}

void
initConcurrentSHLL(ConcurrentSHLL *state)
{
	for (unsigned int i = 0; i < HLL_N_REGISTERS; i++)
		for (unsigned int j = 0; j <= HLL_C_BITS; j++)
			pg_atomic_init_u64(&state->regs[i][j], 0);
}

void
addConcurrentSHLL(ConcurrentSHLL *state, uint32 hash, uint64 now)
{
	uint32		index = hash >> HLL_C_BITS;
	uint8		rank = rho(hash << HLL_BIT_WIDTH, HLL_C_BITS) - 1;
	pg_atomic_uint64 *reg = &state->regs[index][rank];
	uint64		previous = pg_atomic_read_u64(reg);

	while (previous < now && !pg_atomic_compare_exchange_u64(reg, &previous, now))
		;
}

/*
 * Accumulate all windows in one scan. Merge corresponding hit and miss
 * registers before estimating the union, so repeated or overlapping pages
 * cannot inflate combined cardinality.
 */
void
estimateConcurrentSHLLDemand(ConcurrentSHLL *hits, ConcurrentSHLL *misses,
							 uint64 cutoff, SHLLDemandEstimate *estimates)
{
	enum {HITS, MISSES, COMBINED, N_SERIES};
	double		weights[HLL_C_BITS + 2];
	double		sums[N_SERIES][SHLL_DEMAND_WINDOWS] = {{0}};
	unsigned int empties[N_SERIES][SHLL_DEMAND_WINDOWS] = {{0}};

	for (unsigned int rank = 0; rank <= HLL_C_BITS + 1; rank++)
		weights[rank] = ldexp(1.0, -((int) rank));

	for (unsigned int i = 0; i < HLL_N_REGISTERS; i++)
	{
		uint8		first_rank[N_SERIES][SHLL_DEMAND_WINDOWS] = {{0}};

		for (unsigned int j = 0; j <= HLL_C_BITS; j++)
		{
			uint64 hit_second = pg_atomic_read_u64(&hits->regs[i][j]);
			uint64 miss_second = pg_atomic_read_u64(&misses->regs[i][j]);
			uint64 observed[] = {hit_second, miss_second, Max(hit_second, miss_second)};

			for (unsigned int k = 0; k < N_SERIES; k++)
			{
				uint64 age = cutoff - Min(cutoff, observed[k]);

				if (observed[k] != 0 && age < SHLL_DEMAND_WINDOWS * 60)
					first_rank[k][age / 60] = j + 1;
			}
		}

		for (unsigned int k = 0; k < N_SERIES; k++)
		{
			uint8 rank = 0;

			for (unsigned int w = 0; w < SHLL_DEMAND_WINDOWS; w++)
			{
				rank = Max(rank, first_rank[k][w]);
				sums[k][w] += weights[rank];
				empties[k][w] += rank == 0;
			}
		}
	}

	for (unsigned int w = 0; w < SHLL_DEMAND_WINDOWS; w++)
	{
		uint64 cardinalities[N_SERIES];

		for (unsigned int k = 0; k < N_SERIES; k++)
		{
			double estimate = estimateCardinality(sums[k][w], empties[k][w]);

			/* Page hashes have a 32-bit domain, including at saturation. */
			cardinalities[k] = estimate >= 0.0 && estimate < POW_2_32
				? (uint64) round(estimate) : UINT64CONST(4294967296);
		}

		estimates[w].combined = cardinalities[COMBINED];
		estimates[w].hits = cardinalities[HITS];
		estimates[w].misses = cardinalities[MISSES];
	}
}
