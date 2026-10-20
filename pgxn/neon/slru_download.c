/*-------------------------------------------------------------------------
 * slru_download.c -- 将完整远端SLRU快照安全发布到本地缓存。
 *
 * 最新Neon PG的独立下载hook要求扩展落盘，核心随后重新打开文件。本模块
 * 不改变SLRU内容、WAL或事务可见性；只负责完整文件的发布和临时文件清理。
 *-------------------------------------------------------------------------
 */
#include "postgres.h"

#include <unistd.h>

#include "access/slru.h"
#include "common/file_perm.h"
#include "miscadmin.h"
#include "pgstat.h"
#include "storage/fd.h"
#include "storage/ipc.h"
#include "utils/wait_event.h"

#include "slru_download.h"

#if PG_VERSION_NUM >= 170000
#include "utils/injection_point.h"
#else
#define INJECTION_POINT(name) ((void) 0)
#endif

typedef struct NeonSlruPublishCleanup
{
	int			fd;
	bool		owns_temp;
	char		temp[MAXPGPATH];
} NeonSlruPublishCleanup;

static const char *const slru_directories[] = {
	"pg_xact", "pg_multixact/members", "pg_multixact/offsets"
};

/* 仅接受核心SLRU目录中的单一十六进制segment文件，拒绝越目录和任意路径。 */
static bool
neon_slru_path_valid(const char *path)
{
	int			i;

	for (i = 0; i < lengthof(slru_directories); i++)
	{
		Size		prefix = strlen(slru_directories[i]);
		const char *leaf;
		Size		len;

		if (strncmp(path, slru_directories[i], prefix) != 0 || path[prefix] != '/')
			continue;
		leaf = path + prefix + 1;
		len = strlen(leaf);
		return len >= 4 && len <= 15 && strspn(leaf, "0123456789ABCDEF") == len;
	}
	return false;
}

/*-
 * 上下文位于堆上，避免longjmp后读取已改变的自动变量。退出仅关闭自己的
 * transient fd、删除自己的临时路径；永不删除已发布目标，后者可能已被其他
 * Backend打开或更新。FATAL放锁顺序不影响此身份绑定；SIGKILL遗留由新boot清理。
 */
static void
neon_slru_publish_cleanup(int code, Datum arg)
{
	NeonSlruPublishCleanup *cleanup = (NeonSlruPublishCleanup *) DatumGetPointer(arg);

	pgstat_report_wait_end();
	if (cleanup->fd >= 0)
	{
		int			fd = cleanup->fd;

		cleanup->fd = -1;
		(void) CloseTransientFile(fd);
	}
	if (cleanup->owns_temp)
		(void) unlink(cleanup->temp);
}

/*-
 * 调用者已完整接收下载内容。先写同目录独占临时文件并fsync，再用link原子
 * 发布；不能直接写目标（并发读会看到半文件），也不能rename覆盖已有目标
 * （其他SLRU bank的下载/写入者可能已经推进该文件）。EEXIST由现有目标胜出。
 * 同目录保证同一文件系统；整个过程没有Buffer Guard或本模块共享锁。调用者
 * 原生SLRU锁仍遵守核心协议，不能在这里额外获取Buffer/content锁。
 */
void
neon_publish_slru_segment(const char *path, const void *data, int nblocks)
{
	NeonSlruPublishCleanup *cleanup;
	Size		bytes;
	char		directory[MAXPGPATH];
	char	   *slash;

	if (path == NULL || data == NULL || !neon_slru_path_valid(path) ||
		nblocks <= 0 || nblocks > SLRU_PAGES_PER_SEGMENT || strlen(path) >= MAXPGPATH)
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
						errmsg("invalid Neon SLRU segment publication")));
	bytes = (Size) nblocks * BLCKSZ;
	strlcpy(directory, path, sizeof(directory));
	slash = strrchr(directory, '/');
	*slash = '\0';
	cleanup = palloc0(sizeof(*cleanup));
	cleanup->fd = -1;
	PG_ENSURE_ERROR_CLEANUP(neon_slru_publish_cleanup, PointerGetDatum(cleanup));
	{
		uint64		nonce;
		int			attempt;

		CHECK_FOR_INTERRUPTS();
		for (attempt = 0; attempt < 10; attempt++)
		{
			if (!pg_strong_random(&nonce, sizeof(nonce)))
				ereport(ERROR, (errmsg("could not generate Neon SLRU temporary filename")));
			if (snprintf(cleanup->temp, sizeof(cleanup->temp),
						 "%s.neon-slru.%d." UINT64_FORMAT, path, MyProcPid, nonce) >= MAXPGPATH)
				ereport(ERROR, (errcode(ERRCODE_NAME_TOO_LONG),
								errmsg("Neon SLRU temporary filename is too long")));
			cleanup->fd = OpenTransientFilePerm(cleanup->temp,
												O_RDWR | O_CREAT | O_EXCL | PG_BINARY,
												pg_file_create_mode);
			if (cleanup->fd >= 0)
			{
				cleanup->owns_temp = true;
				break;
			}
			if (errno != EEXIST)
				break;
		}
		if (cleanup->fd < 0)
			ereport(ERROR, (errcode_for_file_access(),
							errmsg("could not create Neon SLRU temporary file: %m")));
		INJECTION_POINT("neon-slru-before-write");
		pgstat_report_wait_start(WAIT_EVENT_SLRU_WRITE);
		errno = 0;
		if (pg_pwrite(cleanup->fd, data, bytes, 0) != bytes)
		{
			if (errno == 0)
				errno = ENOSPC;
			ereport(ERROR, (errcode_for_file_access(),
							errmsg("could not write Neon SLRU temporary file: %m")));
		}
		if (pg_fsync(cleanup->fd) != 0)
			ereport(ERROR, (errcode_for_file_access(),
							errmsg("could not sync Neon SLRU temporary file: %m")));
		pgstat_report_wait_end();
		{
			int			fd = cleanup->fd;

			cleanup->fd = -1;
			if (CloseTransientFile(fd) != 0)
				ereport(ERROR, (errcode_for_file_access(),
								errmsg("could not close Neon SLRU temporary file: %m")));
		}
		CHECK_FOR_INTERRUPTS();
		INJECTION_POINT("neon-slru-before-publish");
		if (link(cleanup->temp, path) != 0 && errno != EEXIST)
			ereport(ERROR, (errcode_for_file_access(),
							errmsg("could not publish Neon SLRU file \"%s\": %m", path)));

		/*
		 * 已发布后任何错误都只保留目标；重试以核心重新打开的当前事实为准。
		 */
		INJECTION_POINT("neon-slru-after-publish");
		fsync_fname(directory, true);
		if (unlink(cleanup->temp) != 0)
			ereport(ERROR, (errcode_for_file_access(),
							errmsg("could not remove Neon SLRU temporary file: %m")));
		cleanup->owns_temp = false;
	}
	PG_END_ENSURE_ERROR_CLEANUP(neon_slru_publish_cleanup, PointerGetDatum(cleanup));
	pfree(cleanup);
}

/* 严格识别本模块临时名称，不能让启动清理误删合法segment或其他扩展文件。 */
static bool
neon_slru_is_temporary(const char *name)
{
	const char *marker = strstr(name, ".neon-slru.");
	const char *pid;
	const char *nonce;
	Size		len;

	if (marker == NULL || marker - name < 4 || marker - name > 15 ||
		strspn(name, "0123456789ABCDEF") != marker - name)
		return false;
	pid = marker + strlen(".neon-slru.");
	len = strspn(pid, "0123456789");
	if (len == 0 || pid[len] != '.')
		return false;
	nonce = pid + len + 1;
	return *nonce != '\0' && strspn(nonce, "0123456789") == strlen(nonce);
}

/*-
 * 只在Postmaster建立新共享boot、尚无Backend时清理SIGKILL遗留。共享内存恢复
 * 前旧子进程已被回收；不能在运行中的Backend扫描/删除别人的下载临时文件。
 * 合法segment和未识别文件保留；缺目录不阻止远端basebackup的首次启动。
 */
void
neon_cleanup_slru_downloads(void)
{
	int			i;

	if (IsUnderPostmaster)
		return;
	for (i = 0; i < lengthof(slru_directories); i++)
	{
		DIR		   *dir = AllocateDir(slru_directories[i]);
		struct dirent *entry;

		if (dir == NULL)
		{
			if (errno != ENOENT)
				ereport(WARNING, (errcode_for_file_access(),
								  errmsg("could not open SLRU download directory \"%s\": %m",
										 slru_directories[i])));
			continue;
		}
		while ((entry = ReadDir(dir, slru_directories[i])) != NULL)
		{
			char		path[MAXPGPATH];

			if (!neon_slru_is_temporary(entry->d_name))
				continue;
			snprintf(path, sizeof(path), "%s/%s", slru_directories[i], entry->d_name);
			if (unlink(path) != 0 && errno != ENOENT)
				ereport(WARNING, (errcode_for_file_access(),
								  errmsg("could not remove old Neon SLRU temporary file \"%s\": %m", path)));
		}
		FreeDir(dir);
	}
}
