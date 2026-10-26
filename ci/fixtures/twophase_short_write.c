/*-
 * 只用于独立临时PG集群的故障库，绝不链接/安装到生产PG或镜像。
 * 对显式指定pg_twophase目录中的第一个真实状态文件写入返回半长，保留实际
 * 写入的半文件，再记录本次PID/文件/字节数。用真实短写而非手造损坏文件，
 * 对照旧O_TRUNC路径与新临时文件路径在崩溃恢复时的不同结果。
 * 目录、标记路径均由父测试私有环境传入；未显式配置时所有write原样透传。
 */
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifndef __APPLE__
#include <dlfcn.h>
#include <sys/syscall.h>
static ssize_t (*original_write)(int, const void *, size_t);

__attribute__((constructor)) static void
resolve_original(void)
{
	original_write = (ssize_t (*)(int, const void *, size_t)) dlsym(RTLD_NEXT, "write");
}
#endif

static ssize_t
write_original(int fd, const void *data, size_t count)
{
#ifdef __APPLE__
	/* dyld不会在提供替换函数的本镜像内部再次替换原函数引用。 */
	return write(fd, data, count);
#else
	if (original_write != NULL)
		return original_write(fd, data, count);
	/* 仅覆盖装载器尚未执行constructor的极早阶段，避免dlsym递归。 */
	return syscall(SYS_write, fd, data, count);
#endif
}

static atomic_flag fired = ATOMIC_FLAG_INIT;

static ssize_t
short_twophase_write(int fd, const void *data, size_t count)
{
	const char *directory = getenv("NEON_TEST_TWOPHASE_FAULT_DIR");
	const char *marker = getenv("NEON_TEST_TWOPHASE_FAULT_MARKER");
	char path[PATH_MAX];
	const char *name;
	size_t prefix;

	if (directory == NULL || marker == NULL || directory[0] != '/' || marker[0] != '/' || count < 2)
		return write_original(fd, data, count);
#ifdef __APPLE__
	if (fcntl(fd, F_GETPATH, path) < 0)
		return write_original(fd, data, count);
#else
	{
		char proc[64];
		ssize_t n;
		snprintf(proc, sizeof(proc), "/proc/self/fd/%d", fd);
		n = readlink(proc, path, sizeof(path) - 1);
		if (n < 0 || n == (ssize_t) sizeof(path) - 1)
			return write_original(fd, data, count);
		path[n] = '\0';
	}
#endif
	prefix = strlen(directory);
	if (strncmp(path, directory, prefix) != 0 || path[prefix] != '/')
		return write_original(fd, data, count);
	name = path + prefix + 1;
	if (strspn(name, "0123456789ABCDEF") != 16 || (name[16] != '\0' && strcmp(name + 16, ".tmp") != 0))
		return write_original(fd, data, count);
	if (atomic_flag_test_and_set(&fired))
		return write_original(fd, data, count);

	/* 标记必须在实际短写成功后记录；父测试另核实文件大小和原CHECKPOINT报错。
	 * 不发送信号或杀任何进程，崩溃仅由父测试针对自己的集群执行immediate stop。
	 */
	ssize_t result = write_original(fd, data, count / 2);
	if (result > 0)
	{
		int output = open(marker, O_CREAT | O_EXCL | O_WRONLY, 0600);
		if (output >= 0)
		{
			char body[PATH_MAX + 128];
			int length = snprintf(body, sizeof(body), "%ld\n%s\n%zu\n%zd\n", (long) getpid(), path, count, result);
			if (length > 0 && (size_t) length < sizeof(body))
				(void) write_original(output, body, (size_t) length);
			(void) fsync(output);
			(void) close(output);
		}
	}
	errno = ENOSPC;
	return result;
}

#ifdef __APPLE__
/* Mach-O interpose ABI；绑定格式见Apple dyld的dyld-interposing.h。
 * 只在本测试显式DYLD_INSERT_LIBRARIES时装载，生产二进制不携带此section。
 */
__attribute__((used, section("__DATA,__interpose")))
static const struct { const void *replacement; const void *original; } binding = {
	(const void *) &short_twophase_write, (const void *) &write
};
#else
ssize_t write(int fd, const void *data, size_t count)
{
	return short_twophase_write(fd, data, count);
}
#endif
