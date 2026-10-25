"""真实三级分区索引验证：上游修复要求祖先AX锁持续到事务提交。"""
import os
import unittest

import test_dsg_postgres as lifecycle


@unittest.skipUnless(bool(lifecycle._CONFIGURED_ENV), "需要真实PG与配套库")
class PartitionIndexLockMaintenance(unittest.TestCase):
    def test_ancestor_locks_retained_until_commit_and_concurrent_ddl_waits(self):
        pg = lifecycle.DsgPostgresLifecycle('runTest')
        self.addCleanup(pg.doCleanups)
        pg.setUp()
        pg.start()
        pg.sql("CREATE TABLE p_root(i int) PARTITION BY RANGE(i); "
               "CREATE TABLE p_mid PARTITION OF p_root FOR VALUES FROM(0) TO(100) PARTITION BY RANGE(i); "
               "CREATE TABLE p_leaf PARTITION OF p_mid FOR VALUES FROM(0) TO(100); "
               "CREATE INDEX r_idx ON ONLY p_root(i); CREATE INDEX m_idx ON ONLY p_mid(i); "
               "ALTER INDEX r_idx ATTACH PARTITION m_idx; CREATE INDEX l_idx ON p_leaf(i)")
        def peer():
            p=lifecycle.PgWireConnection(pg.socket,os.environ.get('USER','williamlee'),55432)
            self.addCleanup(p.close)
            return p
        a,b=peer(),peer()
        def query(p,q):return p.exchange([(b'Q',q.encode()+b'\0')])
        def success(msg,status=b'I'):
            self.assertFalse(any(k==b'E' for k,_ in msg),msg)
            self.assertEqual(msg[-1],(b'Z',status))
        success(query(a,'BEGIN; ALTER INDEX m_idx ATTACH PARTITION l_idx'),b'T')
        n=pg.sql(f"SELECT count(*) FROM pg_locks WHERE pid={a.backend_pid} AND granted AND mode='AccessExclusiveLock' AND relation IN ('p_root'::regclass,'r_idx'::regclass)").stdout.strip()
        self.assertEqual(n,'2','ancestor table/index must stay locked after recursive validation')
        success(query(b,"SET lock_timeout='100ms'"))
        blocked=query(b,'ALTER TABLE p_root ADD COLUMN j int')
        self.assertTrue(any(k==b'E' and b'C55P03\0' in body for k,body in blocked),blocked)
        success(query(a,'COMMIT'))
        success(query(b,'ALTER TABLE p_root ADD COLUMN j int'))
        self.assertEqual(pg.sql("SELECT bool_and(indisvalid) FROM pg_index WHERE indexrelid IN('r_idx'::regclass,'m_idx'::regclass,'l_idx'::regclass)").stdout.strip(),'t')

if __name__=='__main__':
    unittest.main(verbosity=2)
