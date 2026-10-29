"""用原main.c函数验证Linux分支控制流；系统调用为明确替身，不认证Linux能力。

通过NEON_TEST_CC显式指定Mac C编译器；提取当前真实函数原文，避免测试复制的
另一套实现。真实Mac PG启动矩阵在test_dsg_address_space.py中独立执行。
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


@unittest.skipUnless(os.environ.get("NEON_TEST_CC"), "需要显式Mac C编译器")
class DsgAddressSpaceControlFlow(unittest.TestCase):
    def test_original_function_checks_privilege_write_and_exact_readback(self):
        source = (Path(__file__).resolve().parents[1] /
                  "vendor/postgres-v17/src/backend/main/main.c").read_text()
        start = source.index("static void\ndsg_apply_inherited_address_space(void)")
        end = source.index("\n\nstatic void startup_hacks", start)
        function = source[start:end]
        harness = r'''
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#define __linux__ 1
#define PR_GET_NO_NEW_PRIVS 39
#define PG_INT64_MAX INT64_MAX
typedef uint64_t uint64;
#define setrlimit mock_setrlimit
#define getrlimit mock_getrlimit
#define getuid mock_getuid
#define geteuid mock_geteuid
static const char *mode;
static struct rlimit recorded;
static unsigned getuid(void) { return strcmp(mode,"root") == 0 ? 0 : 1000; }
static unsigned geteuid(void) { return strcmp(mode,"saved-root") == 0 ? 0 : getuid(); }
static int prctl(int command, long a, long b, long c, long d) {
 if (command != PR_GET_NO_NEW_PRIVS || a || b || c || d) abort();
 return strcmp(mode,"nnp") == 0 ? 0 : 1;
}
static int setrlimit(int resource, const struct rlimit *value) {
 if (resource != RLIMIT_AS || value->rlim_cur != 134217728 || value->rlim_max != 134217728) abort();
 puts("set"); recorded = *value;
 return strcmp(mode,"set-fail") == 0 ? -1 : 0;
}
static int getrlimit(int resource, struct rlimit *value) {
 if (resource != RLIMIT_AS) abort();
 puts("get"); *value = recorded;
 if (strcmp(mode,"hard-drift") == 0) value->rlim_max++;
 if (strcmp(mode,"soft-drift") == 0) value->rlim_cur++;
 return strcmp(mode,"get-fail") == 0 ? -1 : 0;
}
'''
        harness += function
        harness += '\nint main(int argc, char **argv) { if(argc!=2)return 99; mode=argv[1]; dsg_apply_inherited_address_space(); puts("accepted"); return 0; }\n'
        with tempfile.TemporaryDirectory(prefix="dsga-as-c-flow-") as directory:
            root = Path(directory)
            (root / "probe.c").write_text(harness)
            compiled = subprocess.run([os.environ["NEON_TEST_CC"], "-std=c99", "-Wall", "-Wextra",
                            "-Werror", str(root / "probe.c"), "-o", str(root / "probe")],
                           text=True, capture_output=True, timeout=60)
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            environment = os.environ.copy()
            environment["NEON_DSGA_ADDRESS_SPACE_MAX_BYTES"] = "134217728"
            for mode in ("success", "root", "saved-root", "nnp", "set-fail",
                         "get-fail", "hard-drift", "soft-drift"):
                with self.subTest(mode=mode):
                    result = subprocess.run([str(root / "probe"), mode], env=environment,
                                            text=True, capture_output=True, timeout=10)
                    if mode == "success":
                        self.assertEqual(result.returncode, 0)
                        self.assertEqual(result.stdout, "set\nget\naccepted\n")
                        self.assertEqual(result.stderr, "")
                    elif mode in ("root", "saved-root", "nnp"):
                        self.assertEqual(result.returncode, 1)
                        self.assertEqual(result.stdout, "")
                        self.assertIn("unprivileged no-new-privileges birth", result.stderr)
                    else:
                        self.assertEqual(result.returncode, 1)
                        self.assertEqual(result.stdout, "set\n" if mode == "set-fail" else "set\nget\n")
                        self.assertIn("hard limit could not be established", result.stderr)
