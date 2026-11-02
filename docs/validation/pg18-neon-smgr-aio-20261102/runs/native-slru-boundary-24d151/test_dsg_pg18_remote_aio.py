#!/usr/bin/env python3
"""PG18正式Neon远端读与同步AIO桥；本地COPY协议端点不具有Pageserver/WAL预算语义。"""
import contextlib, hashlib, json, os, signal, socket, struct, subprocess, tempfile, threading, time, unittest
from pathlib import Path

BIN = Path(os.environ['NEON_TEST_PG_BIN'])
EVIDENCE = Path(os.environ['NEON_PG18_EVIDENCE'])
PROBE = Path(os.environ['NEON_PG18_AIO_PROBE'])


class PageStream:
    def __init__(self, pages=b'', fail_once=False):
        self.pages, self.fail_once = pages, fail_once
        self.requests, self.errors, self.connections = [], [], []
        self.stop = False
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.port = self.listener.getsockname()[1]
        self.listener.listen(8)
        self.listener.settimeout(.1)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    @staticmethod
    def exact(c, n):
        data = bytearray()
        while len(data) < n:
            part = c.recv(n - len(data))
            if not part:
                raise EOFError()
            data.extend(part)
        return bytes(data)

    def message(self, c):
        kind = self.exact(c, 1)
        size = struct.unpack('!I', self.exact(c, 4))[0]
        if not 4 <= size <= 65536:
            raise RuntimeError('invalid real client PG message size')
        return kind, self.exact(c, size - 4)

    @staticmethod
    def send(c, kind, body):
        c.sendall(kind + struct.pack('!I', len(body) + 4) + body)

    def connection(self, c):
        size = struct.unpack('!I', self.exact(c, 4))[0]
        if not 8 <= size <= 65536:
            raise RuntimeError('invalid real client startup size')
        startup = self.exact(c, size - 4)
        if startup[:4] != struct.pack('!I', 196608):
            raise RuntimeError('expected explicit PGv3 plaintext startup')
        self.send(c, b'R', struct.pack('!I', 0))
        self.send(c, b'S', b'server_version\00018.2\0')
        self.send(c, b'K', struct.pack('!II', 1, 0))
        self.send(c, b'Z', b'I')
        kind, query = self.message(c)
        if kind != b'Q' or not query.startswith(b'pagestream_v3 '):
            raise RuntimeError('expected actual Neon pagestream_v3')
        self.send(c, b'W', b'\0\0\0')
        while not self.stop:
            kind, raw = self.message(c)
            if kind != b'd' or len(raw) < 25:
                raise RuntimeError('expected actual Neon CopyData request')
            tag = raw[0]
            if tag == 2:
                if len(raw) != 42:
                    raise RuntimeError('invalid getpage wire size')
                values = struct.unpack('!BQQQIIIBI', raw)
                block = values[-1]
                self.requests.append({'kind': 'getpage', 'block': block, 'raw_hex': raw.hex()})
                if self.fail_once:
                    self.fail_once = False
                    self.send(c, b'd', bytes([103]) + raw[1:25] + b'forced-pg18-aio-read-error\0')
                else:
                    page = self.pages[block * 8192:(block + 1) * 8192]
                    if len(page) != 8192:
                        raise RuntimeError('real client requested beyond original relation bytes')
                    self.send(c, b'd', bytes([102]) + raw[1:] + page)
            elif tag == 4:
                if len(raw) != 30:
                    raise RuntimeError('invalid Slru request size')
                values = struct.unpack('!BQQQBI', raw)
                if values[-2:] != (0, 10):
                    raise RuntimeError('unexpected Slru exact identity')
                self.requests.append({'kind': 'slru', 'segment': values[-1], 'raw_hex': raw.hex()})
                self.send(c, b'd', bytes([105]) + raw[1:] + struct.pack('!i', 3) + b'M' * (3 * 8192))
            else:
                raise RuntimeError('unexpected actual client request kind ' + str(tag))

    def serve(self):
        while not self.stop:
            try:
                c, _ = self.listener.accept()
            except TimeoutError:
                continue
            except OSError:
                return
            self.connections.append(c)
            c.settimeout(10)
            try:
                self.connection(c)
            except (EOFError, ConnectionResetError, BrokenPipeError):
                pass
            except Exception as e:
                if not self.stop:
                    self.errors.append(repr(e))
            finally:
                c.close()

    def close(self):
        self.stop = True
        self.listener.close()
        for c in self.connections:
            with contextlib.suppress(OSError):
                c.shutdown(socket.SHUT_RDWR)
        self.thread.join(timeout=2)
        if self.thread.is_alive() or self.errors:
            raise AssertionError('fixture terminal error: ' + repr(self.errors))


class RemoteAio(unittest.TestCase):
    @contextlib.contextmanager
    def cluster(self, label):
        evidence = EVIDENCE / label
        evidence.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='pg18-rpc-', dir='/tmp') as temp:
            root = Path(temp)
            data, sock = root / 'data', root / 's'
            sock.mkdir()
            env = {k: v for k, v in os.environ.items() if not k.startswith('NEON_DSGA_')}
            def call(tool, *args, check=True, **kw):
                q = subprocess.run([str(BIN / tool), *map(str, args)], env=env,
                                   text=True, capture_output=True, timeout=30, **kw)
                if check and q.returncode:
                    raise AssertionError(q.stderr)
                return q
            def sql(text, check=True):
                return call('psql', '-h', sock, '-p', '55438', '-U', 'dsg_admin', '-d', 'postgres',
                            '-XqAt', '-v', 'ON_ERROR_STOP=' + str(int(check)), check=check, input=text)
            init = call('initdb', '-D', data, '-U', 'dsg_admin', '-A', 'trust', '--no-locale', '--encoding=UTF8', '--data-checksums', '--no-sync')
            (evidence / 'initdb.log').write_text(init.stdout + init.stderr)
            with (data / 'postgresql.conf').open('a') as f:
                f.write("listen_addresses=''\nport=55438\nunix_socket_directories='" + str(sock) + "'\n"
                        "shared_preload_libraries='neon,neon_rmgr'\nshared_buffers='2MB'\nmax_shared_buffers='64MB'\n"
                        "neon.last_written_lsn_cache_size=1024\n"
                        "neon.tenant_id='11111111111111111111111111111111'\n"
                        "neon.timeline_id='22222222222222222222222222222222'\n"
                        "neon.pageserver_response_log_timeout=500\nneon.pageserver_response_disconnect_timeout=1000\n"
                        "io_method='worker'\nio_workers=2\nhuge_pages=off\nautovacuum=off\n"
                        "io_combine_limit='8kB'\neffective_io_concurrency=0\nmax_parallel_workers_per_gather=0\n")
            stream = (evidence / 'postgres.log').open('w')
            process = subprocess.Popen([str(BIN / 'postgres'), '-D', str(data)], env=dict(env, NEON_DSGA_PG18_NATIVE_ARRAYS='test-v1:8192'), stdout=stream, stderr=subprocess.STDOUT)
            try:
                until = time.monotonic() + 20
                while time.monotonic() < until:
                    if process.poll() is not None:
                        self.fail((evidence / 'postgres.log').read_text())
                    if call('pg_isready', '-h', sock, '-p', '55438', check=False).returncode == 0:
                        break
                    time.sleep(.03)
                else:
                    self.fail('PG18 remote component startup timeout')
                sql("CREATE EXTENSION test_dsg_region18;"
                    f"CREATE FUNCTION test_neon18_remote_setup(regclass) RETURNS void AS '{PROBE}','test_neon18_remote_setup' LANGUAGE C STRICT;"
                    f"CREATE FUNCTION test_neon18_remote_counts() RETURNS text AS '{PROBE}','test_neon18_remote_counts' LANGUAGE C;"
                    f"CREATE FUNCTION test_neon18_slru(text,bigint) RETURNS boolean AS '{PROBE}','test_neon18_slru' LANGUAGE C STRICT;"
                    "REVOKE ALL ON FUNCTION test_neon18_remote_setup(regclass),test_neon18_remote_counts(),test_neon18_slru(text,bigint) FROM PUBLIC;")
                def connect(fixture):
                    value = f'host=127.0.0.1 port={fixture.port} user=fixture dbname=fixture sslmode=disable gssencmode=disable'
                    with (data / 'postgresql.conf').open('a') as f:
                        f.write("\nneon.pageserver_connstring='" + value + "'\n")
                    sql('SELECT pg_reload_conf();')
                    until = time.monotonic() + 3
                    while time.monotonic() < until:
                        if sql('SHOW neon.pageserver_connstring').stdout.strip() == value:
                            return
                        time.sleep(.03)
                    self.fail('actual PM did not consume fixture connstring')
                yield data, sql, connect, evidence
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGINT)
                try:
                    code = process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    code = process.wait(timeout=5)
                stream.close()
                (evidence / 'postmaster-Wait.json').write_text(json.dumps({'PID': process.pid, 'exit': code}) + '\n')
                self.assertEqual(code, 0, (evidence / 'postgres.log').read_text()[-8000:])

    def remote_read(self, fail):
        with self.cluster('remote-error' if fail else 'remote-success') as (data, sql, connect, evidence):
            sql("CREATE TABLE aio18_data(i integer,v text); ALTER TABLE aio18_data ALTER v SET STORAGE PLAIN; INSERT INTO aio18_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i; CHECKPOINT")
            path = sql("SELECT pg_relation_filepath('aio18_data')").stdout.strip()
            pages = (data / path).read_bytes()
            (evidence / 'checkpointed-relation-source.json').write_text(json.dumps({
                'peer_is_model': True, 'model_program': 'ci/test_dsg_pg18_remote_aio.py::PageStream',
                'original_path_inside_isolated_PG': str(data / path), 'relation_path': path,
                'checkpointed_relation_bytes': len(pages), 'checkpointed_relation_sha256': hashlib.sha256(pages).hexdigest(),
                'formal_Pageserver': False, 'storage_E': False}, indent=2) + '\n')
            self.assertGreater(len(pages), 2 << 20)
            fixture = PageStream(pages, fail_once=fail)
            try:
                connect(fixture)
                script = """SELECT 'EVIDENCE '||json_build_object('case','before_pid','value',pg_backend_pid())::text;
SELECT test_neon18_slru('pg_xact/000A',10);
SELECT test_neon18_remote_setup('aio18_data');
SELECT 'EVIDENCE '||json_build_object('case','first_rows','value',json_build_object('rows',count(*),'sum',sum(i)))::text FROM aio18_data;
SELECT 'EVIDENCE '||json_build_object('case','after_first','value',test_dsg_arrays18_snapshot()::json)::text;
SELECT 'EVIDENCE '||json_build_object('case','before_retry','value',test_neon18_remote_counts()::json)::text;
SELECT 'EVIDENCE '||json_build_object('case','retry_rows','value',json_build_object('rows',count(*),'sum',sum(i)))::text FROM aio18_data;
SELECT 'EVIDENCE '||json_build_object('case','after_retry','value',test_dsg_arrays18_snapshot()::json)::text;
SELECT 'EVIDENCE '||json_build_object('case','last_counts','value',test_neon18_remote_counts()::json)::text;
SELECT 'EVIDENCE '||json_build_object('case','after_pid','value',pg_backend_pid())::text;
"""
                q = sql(script, check=False)
                (evidence / 'actual-session.sql').write_text(script)
                (evidence / 'actual-session.stdout').write_text(q.stdout)
                (evidence / 'actual-session.stderr').write_text(q.stderr)
                ev = {}
                for line in q.stdout.splitlines():
                    if line.startswith('EVIDENCE '):
                        item = json.loads(line[len('EVIDENCE '):])
                        self.assertNotIn(item['case'], ev)
                        ev[item['case']] = item['value']
                (evidence / 'actual-evidence.json').write_text(json.dumps(ev, indent=2) + '\n')
                self.assertEqual(q.returncode, 0, q.stderr)
                self.assertEqual(ev['before_pid'], ev['after_pid'])
                self.assertEqual(ev['retry_rows'], {'rows': 600, 'sum': 180300})
                for key in ['after_first', 'after_retry']:
                    self.assertEqual(ev[key]['active_completions'], 0)
                    self.assertEqual(ev[key]['completed_aio'], ev[key]['released_aio_pins'])
                self.assertGreater(int(ev['last_counts']['calls']), 0)
                if fail:
                    self.assertIn('forced-pg18-aio-read-error', q.stderr)
                    self.assertNotIn('first_rows', ev)
                    self.assertEqual(int(ev['before_retry']['errors']), 1)
                    self.assertGreater(int(ev['last_counts']['calls']), int(ev['before_retry']['calls']))
                else:
                    self.assertNotIn('ERROR:', q.stderr)
                    self.assertEqual(ev['first_rows'], {'rows': 600, 'sum': 180300})
                    self.assertEqual(int(ev['last_counts']['errors']), 0)
                self.assertTrue(any(v['kind'] == 'getpage' for v in fixture.requests))
                self.assertFalse(fixture.errors, fixture.errors)
            finally:
                (evidence / 'actual-wire-requests.json').write_text(json.dumps(fixture.requests, indent=2) + '\n')
                fixture.close()

    def test_PG18_S_NEON_REMOTE_AIO_001_real_rpc_success(self):
        self.remote_read(False)

    def test_PG18_S_NEON_REMOTE_AIO_002_real_rpc_ERROR_pin_cleanup_same_backend_retry(self):
        self.remote_read(True)

    def test_PG18_S_NEON_SLRU3_001_real_rpc_and_narrowing_rejections(self):
        with self.cluster('slru-v3') as (data, sql, connect, evidence):
            fixture = PageStream()
            try:
                connect(fixture)
                self.assertEqual(sql("SELECT test_neon18_slru('pg_xact/000A',10)").stdout.strip(), 't')
                self.assertEqual((data / 'pg_xact/000A').read_bytes(), b'M' * (3 * 8192))
                for path, number in [('pg_xact/000A', -1), ('pg_xact/000A', 2147483648), ('pg_xact/000A', 4294967295), ('pg_xact/000A', 4294967296), ('pg_xact/000B', 10)]:
                    q = sql("SELECT test_neon18_slru('" + path + "'," + str(number) + ")", check=False)
                    self.assertIn('ERROR:', q.stderr)
                self.assertEqual(sql("SELECT test_neon18_slru('pg_subtrans/00000000000000A',10)").stdout.strip(), 'f')
                self.assertEqual(len([v for v in fixture.requests if v['kind'] == 'slru']), 1)
            finally:
                (evidence / 'actual-wire-requests.json').write_text(json.dumps(fixture.requests, indent=2) + '\n')
                fixture.close()


if __name__ == '__main__':
    unittest.main(verbosity=2)
