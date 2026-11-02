#!/usr/bin/env python3
"""真实双backend共享AIO等待：协议peer为模型，不是正式Pageserver E。"""
import contextlib, hashlib, json, os, signal, socket, struct, subprocess, tempfile, threading, time, unittest
from pathlib import Path
from test_dsg_pg18_remote_aio import BIN, EVIDENCE, PROBE, PageStream

class Client:
    def __init__(self, path):
        self.c = socket.socket(socket.AF_UNIX); self.c.settimeout(15); self.c.connect(str(path))
        raw = struct.pack('!I',196608)+b'user\0dsg_admin\0database\0postgres\0\0'
        self.c.sendall(struct.pack('!I',len(raw)+4)+raw); self.pid = None; self.receive()
    def receive(self):
        rows, errors = [], []
        while True:
            kind = PageStream.exact(self.c,1); n=struct.unpack('!I',PageStream.exact(self.c,4))[0]
            if not 4 <= n <= 1048576: raise AssertionError('bounded actual PG message')
            raw = PageStream.exact(self.c,n-4)
            if kind == b'K': self.pid=struct.unpack('!II',raw)[0]
            if kind == b'E': errors.append(raw.decode(errors='replace'))
            if kind == b'D':
                count=struct.unpack('!H',raw[:2])[0]; pos=2; row=[]
                for _ in range(count):
                    size=struct.unpack('!i',raw[pos:pos+4])[0];pos+=4
                    row.append(None if size==-1 else raw[pos:pos+size].decode());pos+=max(size,0)
                rows.append(row)
            if kind == b'Z':
                if errors: raise AssertionError(errors)
                return rows
    def query(self, sql):
        PageStream.send(self.c,b'Q',sql.encode()+b'\0'); return self.receive()
    def close(self): self.c.close()

class HeldPageStream(PageStream):
    def __init__(self,pages):
        self.blocked=threading.Event();self.release=threading.Event();self.workers=[]
        super().__init__(pages)
    def send(self,c,kind,body):
        if kind==b'd' and body[:1]==bytes([102]) and not self.blocked.is_set():
            self.blocked.set()
            if not self.release.wait(10):raise AssertionError('bounded original RPC release timeout')
        PageStream.send(c,kind,body)
    def serve(self):
        def serve_one(c):
            try:self.connection(c)
            except (EOFError,ConnectionResetError,BrokenPipeError):pass
            except Exception as e:
                if not self.stop:self.errors.append(repr(e))
            finally:c.close()
        while not self.stop:
            try:c,_=self.listener.accept()
            except TimeoutError:continue
            except OSError:return
            self.connections.append(c);c.settimeout(15)
            thread=threading.Thread(target=serve_one,args=(c,),daemon=True);self.workers.append(thread);thread.start()
    def close(self):
        self.release.set();super().close()
        for t in self.workers:t.join(timeout=2)
        if any(t.is_alive()for t in self.workers):raise AssertionError('model worker not terminal')

class ConcurrentWaiter(unittest.TestCase):
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
                        "neon.pageserver_response_log_timeout=500\nneon.pageserver_response_disconnect_timeout=10000\n"
                        "io_method='io_uring'\nio_workers=2\nhuge_pages=off\nautovacuum=off\n"
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

    def test_PG18_S_AIO_SYNC_WAITER_001_actual_uring_waiter_latch_and_shared_completion(self):
        with self.cluster('io-uring-concurrent-waiter') as (data,sql,connect,evidence):
            self.assertEqual(sql('SHOW io_method').stdout.strip(),'io_uring')
            sql("CREATE TABLE aio18_data(i integer,v text); ALTER TABLE aio18_data ALTER v SET STORAGE PLAIN; INSERT INTO aio18_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i; CHECKPOINT")
            path=sql("SELECT pg_relation_filepath('aio18_data')").stdout.strip();pages=(data/path).read_bytes()
            fixture=HeldPageStream(pages);clients=[];threads=[];results={};errors=[]
            try:
                connect(fixture);socket_path=data.parent/'s/.s.PGSQL.55438'
                owner,waiter,observer=[Client(socket_path)for _ in range(3)];clients=[owner,waiter,observer]
                for client in [owner,waiter]:
                    client.query("SELECT test_neon18_slru('pg_xact/000A',10);SELECT test_neon18_remote_setup('aio18_data')")
                def run(label,client):
                    try:results[label]=client.query('SELECT count(*),sum(i) FROM aio18_data')
                    except Exception as e:errors.append((label,repr(e)))
                first=threading.Thread(target=run,args=('owner',owner));threads.append(first);first.start()
                self.assertTrue(fixture.blocked.wait(5),'owner original remote RPC not held')
                second=threading.Thread(target=run,args=('waiter',waiter));threads.append(second);second.start()
                until=time.monotonic()+5;observations=[]
                while time.monotonic()<until:
                    io=json.loads(observer.query(f"SELECT coalesce(json_agg(row_to_json(a)),'[]') FROM pg_aios a WHERE pid={owner.pid}")[0][0])
                    wait=json.loads(observer.query(f"SELECT row_to_json(a) FROM (SELECT pid,state,wait_event_type,wait_event FROM pg_stat_activity WHERE pid={waiter.pid}) a")[0][0])
                    observations.append({'io':io,'waiter':wait})
                    if any(str(x['state']).lower()=='submitted' and x['f_sync'] is True for x in io) and wait['wait_event']=='AioIoCompletion':break
                    time.sleep(.02)
                else:self.fail('actual synchronous submitted IO plus independent completion waiter not observed: '+repr(observations[-2:]))
                self.assertTrue(first.is_alive()and second.is_alive(),'completion occurred before original peer release')
                (evidence/'held-observations.json').write_text(json.dumps(observations,indent=2)+'\n')
                fixture.release.set()
                for t in threads:t.join(timeout=12)
                self.assertFalse(any(t.is_alive()for t in threads),'real independent waiter stuck after completion')
                self.assertFalse(errors,errors)
                self.assertEqual(results,{'owner':[['600','180300']],'waiter':[['600','180300']]})
                snapshot=json.loads(observer.query('SELECT test_dsg_arrays18_snapshot()')[0][0])
                self.assertEqual(snapshot['active_completions'],0)
                self.assertEqual(snapshot['completed_aio'],snapshot['released_aio_pins'])
                (evidence/'actual-result.json').write_text(json.dumps({'io_method':'io_uring','owner_pid':owner.pid,'waiter_pid':waiter.pid,'results':results,'snapshot':snapshot,'relation_bytes':len(pages),'relation_sha256':hashlib.sha256(pages).hexdigest(),'peer_is_model':True,'Source':False,'E':0},indent=2)+'\n')
            finally:
                fixture.release.set()
                for t in threads:t.join(timeout=15)
                for c in clients:c.close()
                (evidence/'actual-wire-requests.json').write_text(json.dumps(fixture.requests,indent=2)+'\n');fixture.close()

if __name__=='__main__':unittest.main(verbosity=2)
