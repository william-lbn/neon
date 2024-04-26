#ifndef __NEON_UTILS_H__
#define __NEON_UTILS_H__

#include "lib/stringinfo.h"

#ifndef WALPROPOSER_LIB
#include <curl/curl.h>
#endif

bool		HexDecodeString(uint8 *result, char *input, int nbytes);
uint32		pq_getmsgint32_le(StringInfo msg);
uint64		pq_getmsgint64_le(StringInfo msg);
void		pq_sendint32_le(StringInfo buf, uint32 i);
void		pq_sendint64_le(StringInfo buf, uint64 i);
extern void disable_core_dump();

#ifndef WALPROPOSER_LIB

CURL *		alloc_curl_handle(void);

#endif

#endif							/* __NEON_UTILS_H__ */
