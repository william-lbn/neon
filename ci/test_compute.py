import argparse
import hashlib
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch

import compute
import release


class ComputeBuildTests(unittest.TestCase):
    def test_normal_compute_material_checks_published_elf_and_postgres_source(self):
        item = {"image": "docker.io/williamluckyli/compute-node-v17:test", "digest": "sha256:" + "1" * 64}
        expected = compute.LOCK["postgres"]["v17"]

        def record_call(command, **opts):
            if command[1] == "cp":
                destination = command[-1]
                destination.write_bytes(b"\x7fELF" + destination.name.encode())

        with patch.object(compute.subprocess, "run", side_effect=record_call) as run:
            with patch.object(compute.subprocess, "check_output", side_effect=[f"postgres ({expected})", "container\n"]):
                observed = compute.normal_compute_material(item, "v17")
            self.assertEqual(observed, {"postgres_source": expected,
                                        "postgres_elf_sha256": hashlib.sha256(b"\x7fELFpostgres").hexdigest(),
                                        "runtime_elf_sha256": {"compute_ctl": hashlib.sha256(b"\x7fELFcompute_ctl").hexdigest()}})
            self.assertIn(item["image"] + "@" + item["digest"], run.call_args_list[0].args[0])
            with patch.object(compute.subprocess, "check_output", return_value="postgres (old)"):
                with self.assertRaisesRegex(ValueError, "different locked source"):
                    compute.normal_compute_material(item, "v17")

    def test_every_full_extension_exports_its_original_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            for target in compute.extension_targets():
                output = pathlib.Path(directory) / "Dockerfile"
                compute.render(argparse.Namespace(pg="v17", target=target, output=output, artifacts=directory, payload=True))
                text = output.read_text()
                self.assertIn("FROM scratch AS payload", text)
                self.assertIn(f"COPY --from={target} /usr/local/pgsql/", text)
                self.assertIn(" /ext-src/ /ext-src/", text)
                self.assertNotIn("--default-toolchain stable", text)
                self.assertIn('RUN bash /usr/local/bin/neon-apt-sources "$DEBIAN_VERSION"', text)
                if target == "postgis-build":
                    self.assertIn("COPY --from=postgis-build /sfcgal/", text)
                if target == "h3-pg-build":
                    self.assertIn("COPY --from=h3-pg-build /h3/", text)

    def test_digest_import_removes_compilation_but_keeps_build_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory)
            digest = "sha256:" + "a" * 64
            (path / "pg.json").write_text(json.dumps({"kind": "compute-stage", "pg": "v17", "aliases": ["pg-build"], "image": "docker.io/williamluckyli/neon-compute-build-v17:test-pg-build", "digest": digest}))
            output = path / "Dockerfile"
            compute.render(argparse.Namespace(pg="v17", target="runtime", output=output, artifacts=path, payload=False))
            text = output.read_text()
            self.assertIn(f"@{digest} AS pg-build\nARG PG_VERSION\nARG DEBIAN_VERSION", text)
            self.assertNotIn("COPY vendor/postgres-${PG_VERSION:?} postgres", text)
            self.assertIn("FROM extensions-${EXTENSIONS} AS neon-pg-ext-build", text)
            self.assertIn("COPY --from=neon-pg-ext-build /usr/local/pgsql", text)
            self.assertIn("ENTRYPOINT [\"/usr/local/bin/compute_ctl\"]", text)

    def test_conflicting_stage_digests_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            for i in range(2):
                item = {"kind": "compute-stage", "pg": "v17", "aliases": ["pg-build"], "image": "docker.io/williamluckyli/build:test", "digest": "sha256:" + str(i) * 64}
                (pathlib.Path(directory) / f"{i}.json").write_text(json.dumps(item))
            with self.assertRaisesRegex(ValueError, "Conflicting stage"):
                compute.records(directory)

    def test_release_requires_all_versions_and_proxy_services(self):
        names = release.expected_images()
        for version in ["v14", "v15", "v16", "v17"]:
            for prefix in ["compute-node", "vm-compute-node", "neon-test-extensions"]:
                self.assertIn(f"{prefix}-{version}", names)
        for name in ["proxy", "local-proxy", "pg-sni-router", "storage-controller", "vm-monitor", "neonvm-daemon", "cluster-autoscaler-neonvm"]:
            self.assertIn(name, names)

    def test_old_pg_harness_does_not_reference_missing_upstream_patches(self):
        with tempfile.TemporaryDirectory() as directory:
            for version in ["v14", "v15", "v16", "v17"]:
                output = pathlib.Path(directory) / "Dockerfile"
                compute.render(argparse.Namespace(pg=version, target="extension-tests", output=output, artifacts=directory, payload=False))
                block = output.read_text().split(" AS extension-tests\n", 1)[1].split("\nFROM ", 1)[0]
                self.assertEqual("pg_hint_plan_${PG_VERSION:?}.patch" in block, version in ["v16", "v17"])


if __name__ == "__main__":
    unittest.main()
