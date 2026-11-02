"""正式PG18 WAL生产→原redo Apply；私有shim仅验证新inmem AIO入口。"""
from pathlib import Path
import hashlib,json,os,re,signal,struct,subprocess,tempfile,time
E=Path('/evidence');B=Path('/opt/pg18/bin');results=[]
assert 'LD_LIBRARY_PATH' not in os.environ
def call(tool,*args,input=None,check=True):
 q=subprocess.run([str(B/tool),*map(str,args)],input=input,text=True,capture_output=True,timeout=30)
 if check and q.returncode:raise AssertionError(q.stderr)
 return q
def lsn(s):a,b=s.split('/');return(int(a,16)<<32)|int(b,16)
def crc32c(b):
 c=0xffffffff
 for x in b:
  c^=x
  for _ in range(8):c=(c>>1)^(0x82f63b78 if c&1 else 0)
 return c^0xffffffff
with tempfile.TemporaryDirectory(prefix='pg18-actual-wal-',dir='/tmp')as t:
 root=Path(t);data=root/'data';sock=root/'sock';sock.mkdir()
 q=call('initdb','-D',data,'-U','dsg_admin','-A','trust','--no-locale','--encoding=UTF8','--no-sync','--no-data-checksums');(E/'initdb.log').write_text(q.stdout+q.stderr)
 (data/'postgresql.conf').write_text((data/'postgresql.conf').read_text()+"\nlisten_addresses=''\nport=55438\nunix_socket_directories='"+str(sock)+"'\nshared_preload_libraries='neon,neon_rmgr'\nshared_buffers='2MB'\nmax_shared_buffers='64MB'\nneon.last_written_lsn_cache_size=1024\nneon.tenant_id='11111111111111111111111111111111'\nneon.timeline_id='22222222222222222222222222222222'\nhuge_pages=off\nautovacuum=off\nwal_compression=off\nfull_page_writes=on\n")
 log=(E/'postgres.log').open('wb');pm=subprocess.Popen([str(B/'postgres'),'-D',str(data)],env=dict(os.environ,NEON_DSGA_PG18_NATIVE_ARRAYS='test-v1:8192'),stdout=log,stderr=subprocess.STDOUT)
 def sql(s,check=True):return call('psql','-h',sock,'-p','55438','-U','dsg_admin','-d','postgres','-XqAt','-v','ON_ERROR_STOP='+str(int(check)),'-v','VERBOSITY=verbose',input=s,check=check)
 try:
  until=time.monotonic()+20
  while time.monotonic()<until:
   if pm.poll()is not None:raise AssertionError((E/'postgres.log').read_text())
   if call('pg_isready','-h',sock,'-p','55438',check=False).returncode==0:break
   time.sleep(.03)
  else:raise AssertionError('actual WAL producer startup timeout')
  sql("CREATE FUNCTION pg18_redo_vectors(integer) RETURNS boolean AS '/evidence/pg18_redo_vectors.so','pg18_redo_vectors' LANGUAGE C STRICT;")
  q=sql("SELECT pg_backend_pid(); SELECT pg18_redo_vectors(3); SELECT pg18_redo_vectors(4); SELECT pg18_redo_vectors(3); SELECT pg_backend_pid();",check=False)
  (E/'actual-aio.stdout').write_text(q.stdout);(E/'actual-aio.stderr').write_text(q.stderr);parts=q.stdout.splitlines();assert q.returncode==0 and len(parts)==4 and parts[0]==parts[3] and parts[1:3]==['t','t'] and'22023'in q.stderr,(q.stdout,q.stderr)
  results.append({'id':'PG18_S_REDO_STARTREADV_001','PASS':True,'actual_PgAioHandle':True,'actual_callback':'production inmem_startreadv via test-only included source','actual_completion_result':2,'distinct_blocks':[42,43],'same_backend_after_invalid_batch':int(parts[0]),'error_SQLSTATE':'22023','no_fake_FD':True})
  (E/'targets.json').write_text(json.dumps({'targets':results,'Source':False,'normal':False,'E':0},indent=2)+'\n')
  sql("CREATE TABLE redo_actual_wal(i int,v text); INSERT INTO redo_actual_wal VALUES(1,'before'); CHECKPOINT;")
  relpath=sql("SELECT pg_relation_filepath('redo_actual_wal');").stdout.strip();assert relpath.startswith('base/')and'..'not in Path(relpath).parts
  db,rel=map(int,Path(relpath).parts[1:]);basepage=(data/relpath).read_bytes()[:8192];assert len(basepage)==8192
  start=sql('SELECT pg_current_wal_insert_lsn();').stdout.strip();sql("UPDATE redo_actual_wal SET v=repeat('after',100) WHERE i=1;");stop=sql('SELECT pg_current_wal_insert_lsn();').stdout.strip();sql('CHECKPOINT;');expected=(data/relpath).read_bytes()[:8192];assert basepage!=expected
  q=call('pg_waldump','-p',data/'pg_wal','-s',start,'-e',stop);(E/'actual-pg-waldump.txt').write_text(q.stdout+q.stderr)
  matches=[x for x in q.stdout.splitlines()if f'rel 1663/{db}/{rel}'in x and'blk 0'in x and'FPW'in x and('UPDATE'in x or'UPDATE+INIT'in x)];assert len(matches)==1,q.stdout
  record_lsn=re.search(r'lsn:\s*([0-9A-Fa-f]+/[0-9A-Fa-f]+)',matches[0]).group(1);pos=lsn(record_lsn);segsize=int(sql('SHOW wal_segment_size').stdout.strip().removesuffix('MB'))<<20;assert segsize==16<<20
  def raw_at(pos,n):
   segno=pos//segsize;name=f'00000001{segno//(0x100000000//segsize):08X}{segno%(0x100000000//segsize):08X}'
   with(data/'pg_wal'/name).open('rb')as f:f.seek(pos%segsize);return f.read(n)
  total=struct.unpack('<I',raw_at(pos,4))[0];assert 24<=total<=65536;raw=bytearray();left=total
  while left:
   take=min(left,8192-pos%8192);part=raw_at(pos,take);assert len(part)==take;raw+=part;pos+=take;left-=take
   if left:
    header=raw_at(pos,40);magic,info=struct.unpack_from('<HH',header);assert magic!=0 and info&1;pos+=40 if info&2 else 24
  raw=bytes(raw);assert crc32c(raw[24:]+raw[:20])==struct.unpack_from('<I',raw,20)[0]
  hi,lo=struct.unpack_from('<II',expected);end_lsn=(hi<<32)|lo;assert pos<=end_lsn<=((pos+7)&~7),(hex(pos),hex(end_lsn))
  for name,blob in [('actual-base-page.bin',basepage),('actual-expected-page.bin',expected),('actual-WAL-record.bin',raw)]: (E/name).write_bytes(blob)
  (E/'actual-WAL-source.json').write_text(json.dumps({'producer':'formal PG18 postgres SQL UPDATE after CHECKPOINT','record_start_lsn':record_lsn,'record_end_lsn':f'{end_lsn>>32:X}/{end_lsn&0xffffffff:X}','raw_record_bytes':len(raw),'raw_record_sha256':hashlib.sha256(raw).hexdigest(),'CRC32C_valid':True,'source_relation':[1663,db,rel],'block':0,'full_page_image':True,'data_checksums':'fixture explicitly disabled for byte-for-byte memory page comparison','Source':False,'normal':False,'E':0},indent=2)+'\n')
 finally:
  if pm.poll()is None:pm.send_signal(signal.SIGINT)
  try:rc=pm.wait(timeout=10)
  except subprocess.TimeoutExpired:pm.kill();rc=pm.wait(timeout=5)
  log.close();(E/'postmaster-Wait.json').write_text(json.dumps({'PID':pm.pid,'exit':rc})+'\n');assert rc==0
def frame(kind,payload=b''):return kind+struct.pack('!I',len(payload)+4)+payload
def tag(block):return struct.pack('!BIIII',0,1663,db,rel,block)
payload=frame(b'B',tag(0))+frame(b'P',tag(0)+basepage)
for i in range(1,7):
 page=bytearray(8192);struct.pack_into('=HHHH',page,12,24,8192,8192,8192|4);page[24:]=bytes([i])*(8192-24);payload+=frame(b'P',tag(i)+page)
payload+=frame(b'B',tag(0))+frame(b'A',struct.pack('!Q',end_lsn)+raw)+frame(b'G',tag(0))
q=subprocess.run([str(B/'postgres'),'--wal-redo'],input=payload,capture_output=True,timeout=30,cwd='/tmp');(E/'redo-apply.stderr').write_bytes(q.stderr);(E/'redo-apply-Wait.json').write_text(json.dumps({'exit':q.returncode,'stdin_bytes':len(payload),'stdin_sha256':hashlib.sha256(payload).hexdigest(),'stdout_bytes':len(q.stdout),'pages_pushed':7,'local_buffers':4})+'\n');(E/'actual-redo-result-page.bin').write_bytes(q.stdout)
assert q.returncode==0,q.stderr.decode(errors='replace');assert q.stdout==expected,'actual PG18 WAL Apply/Get differs from actual PG18 final page'
results.append({'id':'PG18_S_FORMAL_WAL_APPLY_001','PASS':True,'formal_PG18_record':True,'ApplyRecord_actual':True,'pages_pushed':7,'local_buffers':4,'expected_page_source':'same actual PG18 SQL UPDATE and checkpoint','byte_for_byte_equal':True,'walredo_cache_path':'localbuf synchronous smgr read/write; not startreadv','formal_Pageserver':False})
(E/'targets.json').write_text(json.dumps({'targets':results,'Source':False,'normal':False,'E':0},indent=2)+'\n');print(json.dumps({'PASS':len(results),'targets':[x['id']for x in results]}))
