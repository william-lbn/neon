\set ON_ERROR_STOP on
CREATE EXTENSION neon VERSION '1.8';
CREATE TABLE dsg_data(i integer PRIMARY KEY,pad text);
ALTER TABLE dsg_data ALTER pad SET STORAGE PLAIN;
INSERT INTO dsg_data SELECT i,repeat(md5(i::text),150) FROM generate_series(1,600) i;
CREATE TEMP TABLE dsg_requests(rev integer PRIMARY KEY, request jsonb);
CREATE TEMP TABLE dsg_evidence(label text PRIMARY KEY, value jsonb);
INSERT INTO dsg_evidence VALUES('capabilities',neon_dsg_capabilities()::jsonb);
INSERT INTO dsg_evidence SELECT 'initial',neon_dsg_memory_status()::jsonb;
INSERT INTO dsg_evidence SELECT 'data_before',jsonb_build_object('rows',count(*),'bytes',pg_relation_size('dsg_data'),'checksum',md5(string_agg(i::text||':'||pad,',' ORDER BY i))) FROM dsg_data;
CREATE FUNCTION pg_temp.req(rev integer,gen integer,target integer,kind text) RETURNS jsonb LANGUAGE SQL AS $$
 SELECT jsonb_build_object('protocol_version',1,'key',jsonb_build_object('instance',neon_dsg_capabilities()::jsonb->'instance','issuer_epoch','1','revision',rev::text,'operation_id',jsonb_build_object('high','18364758544493064720','low',rev::text)),'kind',kind,'expected_capacity_generation',gen::text,'target_buffers',target,'target_vm_memory_bytes','1073741824','budget_commitment_id','linux-formal-sql-component-only','deadline_unix_ms','18446744073709551615')
$$;
INSERT INTO dsg_requests VALUES(1,pg_temp.req(1,1,1024,'grow'));
INSERT INTO dsg_evidence SELECT 'grow',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=1;
INSERT INTO dsg_evidence SELECT 'data_after_grow',jsonb_build_object('rows',count(*),'checksum',md5(string_agg(i::text||':'||pad,',' ORDER BY i))) FROM dsg_data;
INSERT INTO dsg_requests VALUES(2,pg_temp.req(2,2,256,'shrink'));
INSERT INTO dsg_evidence SELECT 'shrink',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=2;
INSERT INTO dsg_evidence SELECT 'data_after_shrink',jsonb_build_object('rows',count(*),'checksum',md5(string_agg(i::text||':'||pad,',' ORDER BY i))) FROM dsg_data;
INSERT INTO dsg_requests VALUES(3,pg_temp.req(3,3,2048,'grow'));
INSERT INTO dsg_evidence SELECT 'regrow',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=3;
INSERT INTO dsg_evidence SELECT 'data_after_regrow',jsonb_build_object('rows',count(*),'checksum',md5(string_agg(i::text||':'||pad,',' ORDER BY i))) FROM dsg_data;
INSERT INTO dsg_evidence SELECT 'replay',neon_dsg_request_resize(request::text)::jsonb FROM dsg_requests WHERE rev=1;
INSERT INTO dsg_evidence SELECT 'historical_status',neon_dsg_resize_status(jsonb_build_object('protocol_version',1,'key',request->'key')::text)::jsonb FROM dsg_requests WHERE rev=1;
INSERT INTO dsg_evidence SELECT 'same_key_conflict',neon_dsg_request_resize(jsonb_set(request,'{target_buffers}','512'::jsonb)::text)::jsonb FROM dsg_requests WHERE rev=1;
INSERT INTO dsg_evidence VALUES('expired',neon_dsg_request_resize(jsonb_set(pg_temp.req(4,4,4096,'grow'),'{deadline_unix_ms}','"1"'::jsonb)::text)::jsonb);
INSERT INTO dsg_evidence SELECT 'final',neon_dsg_memory_status()::jsonb;
SELECT 'DSG_SQL_EVIDENCE '||jsonb_build_object('case',label,'value',value)::text FROM dsg_evidence ORDER BY label;
