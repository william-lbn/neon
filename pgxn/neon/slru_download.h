#ifndef NEON_SLRU_DOWNLOAD_H
#define NEON_SLRU_DOWNLOAD_H

#include "storage/smgr.h"

#if defined(NEON_SLRU_DOWNLOAD_API_VERSION) && NEON_SLRU_DOWNLOAD_API_VERSION != 2
#error "unsupported Neon SLRU download API: adapt the storage protocol explicitly"
#endif

/* 内部C接口；没有对应的普通用户SQL入口。仅发布完整下载的SLRU文件。 */
extern PGDLLEXPORT void neon_publish_slru_segment(const char *path,
												  const void *data, int nblocks);
extern void neon_cleanup_slru_downloads(void);

#if defined(NEON_SLRU_DOWNLOAD_API_VERSION) && NEON_SLRU_DOWNLOAD_API_VERSION == 2
extern PGDLLEXPORT bool neon_download_slru_segment(const char *path, int segno);
#endif

#endif
