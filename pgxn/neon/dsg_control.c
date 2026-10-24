/*-------------------------------------------------------------------------
 * dsg_control.c -- Neon内部DSGA管理协议与内核控制器之间的边界。
 *
 * 普通租户/弱superuser无权调用，即使SQL ACL或SECURITY DEFINER被误配置。
 * 本层验证规范wire、固定实例身份和完整意图绑定；资源承诺内容/issuer所有权
 * 由认证管理执行链核验。本层只返回PG/Provider事实，从不发放VM缩容许可。
 *-------------------------------------------------------------------------
 */
#include "postgres.h"

#include "access/sysattr.h"
#include "catalog/pg_authid.h"
#include "common/cryptohash.h"
#include "common/int.h"
#include "common/jsonapi.h"
#include "fmgr.h"
#include "executor/executor.h"
#include "lib/stringinfo.h"
#include "miscadmin.h"
#include "utils/acl.h"
#include "utils/builtins.h"
#include "utils/guc.h"
#include "utils/json.h"
#include "utils/jsonb.h"
#include "utils/jsonfuncs.h"
#include "utils/timestamp.h"

#include "dsg_control.h"

/*- 其他官方PG版本未移植本能力时仍能加载Neon，明确协商为不支持。 */
#if defined(__has_include)
#if __has_include("storage/dynamic_shared_buffers.h")
#include "storage/dynamic_shared_buffers.h"
#endif
#if __has_include("storage/dsg_work.h")
#include "storage/dsg_work.h"
#endif
#endif
#if defined(NEON_DSG_BUFFER_API_VERSION) && NEON_DSG_BUFFER_API_VERSION == 6
#define NEON_DSG_CONTROL_SUPPORTED 1
#endif
#if defined(NEON_DSG_CONTROL_SUPPORTED) && defined(NEON_DSG_ADMISSION_API_VERSION) && NEON_DSG_ADMISSION_API_VERSION == 1
#define NEON_DSG_ADMISSION_SUPPORTED 1
#endif

#define NEON_DSG_MAX_ID_BYTES 128
#define NEON_DSG_MAX_REQUEST_BYTES 16384

PG_FUNCTION_INFO_V1(neon_dsg_capabilities);
PG_FUNCTION_INFO_V1(neon_dsg_request_resize);
PG_FUNCTION_INFO_V1(neon_dsg_resize_status);
PG_FUNCTION_INFO_V1(neon_dsg_cancel_resize);
PG_FUNCTION_INFO_V1(neon_dsg_memory_status);
PG_FUNCTION_INFO_V1(neon_dsg_fence_issuer);
PG_FUNCTION_INFO_V1(neon_dsg_set_admission);
PG_FUNCTION_INFO_V1(neon_dsg_admission_status);
PG_FUNCTION_INFO_V1(neon_dsg_shared_cost);
PG_FUNCTION_INFO_V1(neon_dsg_work_state);

static char *dsg_vm_uid;
static char *dsg_compute_instance_id;
static bool dsg_initializing_identity;
static bool dsg_rejected_startup_identity;

#ifdef NEON_DSG_CONTROL_SUPPORTED
static ExecutorCheckPerms_hook_type dsg_previous_check_perms;

/*-
 * Neon弱组继承pg_read_all_data，普通NOSUPERUSER仍可能读取pg_authid密码。
 * 在实际列权限检查处保护verifier，不能只依赖关系ACL或禁止直接表名：
 * pg_shadow/SECURITY DEFINER的checkAsUser可能是超级用户，但原会话仍是租户。
 * 同时检查当前身份和session身份。pg_roles使用常量掩码，不引用密码列，保持
 * 标准角色元数据可读；整行/复合值则包含密码，必须拒绝。只对已启用DSGA生效，
 * 旧Neon与其他PG版本不改变权限语义；仍调用原hook，不能覆盖其他扩展的限制。
 * 同时拒绝租户经SQL执行器写认证目录（含错误配置的SQL SECURITY DEFINER）；
 * 正常ALTER ROLE仍由PG自身的utility权限检查负责。本hook不是物理复制、WAL、
 * 备份或任意不安全扩展的全通道保护，不能据此免除私有管理/HBA/发布来源门禁。
 */
static bool
NeonDsgCheckAuthenticationSecrets(List *range_table, List *permission_infos,
								bool report_error)
{
	if (enable_dynamic_shared_buffers &&
		(!superuser() || !superuser_arg(GetSessionUserId())))
	{
		ListCell   *cell;

		foreach(cell, permission_infos)
		{
			RTEPermissionInfo *info = lfirst_node(RTEPermissionInfo, cell);

			if (info->relid == AuthIdRelationId &&
				((info->requiredPerms & (ACL_INSERT | ACL_UPDATE | ACL_DELETE | ACL_TRUNCATE)) ||
				 ((info->requiredPerms & ACL_SELECT) &&
				 (bms_is_member(Anum_pg_authid_rolpassword - FirstLowInvalidHeapAttributeNumber,
							   info->selectedCols) ||
				 bms_is_member(InvalidAttrNumber - FirstLowInvalidHeapAttributeNumber,
							   info->selectedCols)))))
			{
				if (report_error)
					ereport(ERROR,
							(errcode(ERRCODE_INSUFFICIENT_PRIVILEGE),
							 errmsg("permission denied for protected authentication secrets")));
				return false;
			}
		}
	}
	if (dsg_previous_check_perms)
		return dsg_previous_check_perms(range_table, permission_infos, report_error);
	return true;
}
#endif

/*- 所有ID使用相同ASCII字节规则，不能用locale字符分类或截断超长输入。 */
static bool
NeonDsgValidId(const char *value, Size length)
{
	Size		i;

	if (length == 0 || length > NEON_DSG_MAX_ID_BYTES)
		return false;
	for (i = 0; i < length; i++)
		if ((unsigned char) value[i] < 33 || (unsigned char) value[i] > 126)
			return false;
	return true;
}

static bool
NeonDsgCheckIdentity(char **value, void **extra, GucSource source)
{
	/*- 空默认值表示未受管绑定；管理写入会拒绝，不能把名字猜成实例UID。 */
	if (*value != NULL && (**value == '\0' || NeonDsgValidId(*value, strlen(*value))))
		return true;
	if (dsg_initializing_identity)
		dsg_rejected_startup_identity = true;
	GUC_check_errdetail("DSGA identities require 1..128 visible ASCII bytes.");
	return false;
}

void
pg_init_dsg_control(void)
{
#ifdef NEON_DSG_CONTROL_SUPPORTED
	dsg_previous_check_perms = ExecutorCheckPerms_hook;
	ExecutorCheckPerms_hook = NeonDsgCheckAuthenticationSecrets;
#endif
	/*-
	 * 仅Postmaster配置，不能经普通SET/SIGHUP改变绑定。Compute必须用-c
	 * 同时覆盖备份auto.conf/include；PG boot由内核随机生成，不能外部指定。
	 */
	dsg_initializing_identity = true;
	dsg_rejected_startup_identity = false;
	DefineCustomStringVariable("neon.dsg_vm_uid", "Managed DSGA VM UID", NULL,
							   &dsg_vm_uid, "", PGC_POSTMASTER, GUC_NOT_IN_SAMPLE,
							   NeonDsgCheckIdentity, NULL, NULL);
	DefineCustomStringVariable("neon.dsg_compute_instance_id", "Managed DSGA compute instance", NULL,
							   &dsg_compute_instance_id, "", PGC_POSTMASTER, GUC_NOT_IN_SAMPLE,
							   NeonDsgCheckIdentity, NULL, NULL);
	dsg_initializing_identity = false;
	/*-
	 * preload前的custom GUC是占位符。原生重新应用占位值时，check hook
	 * 拒绝可能只产生WARNING并回退默认，不能靠返回false保证启动失败。
	 * hook保持原生错误协议；注册结束后显式拒绝已见到的非法启动身份，
	 * 防止静默回退成空值/旧值。普通backend延迟加载使用ERROR而非杀进程。
	 */
	if (dsg_rejected_startup_identity)
		ereport(IsUnderPostmaster ? ERROR : FATAL,
				(errcode(ERRCODE_INVALID_PARAMETER_VALUE),
				 errmsg("invalid DSGA managed startup identity"),
				 errdetail("DSGA identities require 1..128 visible ASCII bytes.")));
}

static void
NeonDsgCheckAdministrator(void)
{
	/*-
	 * Neon cloud_admin是真正PG superuser，租户neon_superuser不是。
	 * 同时核验会话身份，阻止租户借误授予的SECURITY DEFINER间接调用。
	 */
	if (!superuser() || !superuser_arg(GetSessionUserId()))
		ereport(ERROR, (errcode(ERRCODE_INSUFFICIENT_PRIVILEGE),
						errmsg("DSGA management requires a superuser session")));
}

static Datum
NeonDsgJson(StringInfo output)
{
	return DirectFunctionCall1(jsonb_in, CStringGetDatum(output->data));
}

#ifndef NEON_DSG_CONTROL_SUPPORTED
static void
NeonDsgUnsupported(void)
{
	ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
					errmsg("this Neon PostgreSQL build does not support DSGA control protocol 1")));
}
#endif

#ifdef NEON_DSG_CONTROL_SUPPORTED

typedef struct NeonDsgIntent
{
	DsgBufferResizeRequest request;
	DsgBufferRequestKey key;
	DsgBufferOperationKind kind;
	char	   *vm_uid;
	char	   *compute_instance_id;
	char	   *budget_commitment_id;
	uint64		target_vm_memory_bytes;
	uint64		deadline_unix_ms;
} NeonDsgIntent;

/*- 只从已检查唯一键的对象取成员，不允许缺失/NULL/隐式数字转字符串。 */
static JsonbValue *
NeonDsgField(JsonbContainer *object, const char *name, enum jbvType type)
{
	JsonbValue	key;
	JsonbValue *value;

	key.type = jbvString;
	key.val.string.val = (char *) name;
	key.val.string.len = strlen(name);
	value = findJsonbValueFromContainer(object, JB_FOBJECT, &key);
	if (value == NULL || value->type != type)
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
						errmsg("invalid DSGA field \"%s\"", name)));
	return value;
}

static void
NeonDsgObject(JsonbContainer *object, int count)
{
	/*- 每个预期字段随后均按名字读取，成员数相同仍不能用未知成员替代必需项。 */
	if (!JsonContainerIsObject(object) || JsonContainerSize(object) != count)
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
						errmsg("DSGA object fields do not match control protocol 1")));
}

static JsonbContainer *
NeonDsgChild(JsonbContainer *object, const char *name, int count)
{
	JsonbValue *child = NeonDsgField(object, name, jbvBinary);
	JsonbContainer *result = child->val.binary.data;

	NeonDsgObject(result, count);
	return result;
}

static uint64
NeonDsgUint64(JsonbContainer *object, const char *name)
{
	JsonbValue *value = NeonDsgField(object, name, jbvString);
	uint64		result;

	if (!DsgBufferParseUint64(value->val.string.val, value->val.string.len, &result))
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
						errmsg("DSGA field \"%s\" requires a canonical uint64 string", name)));
	return result;
}

static char *
NeonDsgId(JsonbContainer *object, const char *name)
{
	JsonbValue *value = NeonDsgField(object, name, jbvString);

	if (!NeonDsgValidId(value->val.string.val, value->val.string.len))
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
						errmsg("invalid DSGA identity \"%s\"", name)));
	return pnstrdup(value->val.string.val, value->val.string.len);
}

/*-
 * JSONB会把1e0规范化为1；先检查原始数字token，保证与Rust u32语法一致。
 * uint64只能是字符串，协议里仅version/target是JSON整数，负号/小数/指数拒绝。
 */
static JsonParseErrorType
NeonDsgIntegerToken(void *state, char *token, JsonTokenType type)
{
	const char *p;

	if (type != JSON_TOKEN_NUMBER)
	{
		if (token != NULL)
			pfree(token);
		return JSON_SUCCESS;
	}
	if (strlen(token) > 10)
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("invalid DSGA integer token")));
	for (p = token; *p; p++)
		if (*p < '0' || *p > '9')
			ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("invalid DSGA integer token")));
	pfree(token);
	return JSON_SUCCESS;
}

static uint64
NeonDsgInteger(JsonbContainer *object, const char *name)
{
	JsonbValue *value = NeonDsgField(object, name, jbvNumeric);
	char	   *text_value = DatumGetCString(DirectFunctionCall1(numeric_out, NumericGetDatum(value->val.numeric)));
	uint64		result;

	if (!DsgBufferParseUint64(text_value, strlen(text_value), &result))
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("invalid DSGA integer field \"%s\"", name)));
	return result;
}

static void
NeonDsgHashIntent(Jsonb *input, unsigned char *output)
{
	char	   *canonical = JsonbToCString(NULL, &input->root, VARSIZE(input));
	pg_cryptohash_ctx *hash = pg_cryptohash_create(PG_SHA256);
	bool		success;

	if (hash == NULL)
		ereport(ERROR, (errcode(ERRCODE_OUT_OF_MEMORY), errmsg("could not allocate DSGA intent hash")));
	success = pg_cryptohash_init(hash) == 0 &&
		pg_cryptohash_update(hash, (const uint8 *) canonical, strlen(canonical)) == 0 &&
		pg_cryptohash_final(hash, output, DSG_BUFFER_CONTEXT_BYTES) == 0;
	pg_cryptohash_free(hash);
	if (!success)
		ereport(ERROR, (errcode(ERRCODE_INTERNAL_ERROR), errmsg("could not hash DSGA intent")));
	/*- 哈希是本boot内完整语义意图的绑定，不是外部签名/认证或预算许可。 */
}

/*- 所有入口共用原始长度/整数token/唯一键校验，禁止先归一化后才检查。 */
static Jsonb *
NeonDsgParseRaw(text *raw)
{
	JsonLexContext lex;
	JsonSemAction sem = {0};

	if (VARSIZE_ANY_EXHDR(raw) > NEON_DSG_MAX_REQUEST_BYTES)
		ereport(ERROR, (errcode(ERRCODE_PROGRAM_LIMIT_EXCEEDED), errmsg("DSGA request exceeds 16 KiB")));
	makeJsonLexContext(&lex, raw, true);
	sem.scalar = NeonDsgIntegerToken;
	pg_parse_json_or_ereport(&lex, &sem);
	freeJsonLexContext(&lex);
	/*-
	 * true要求解析过程中检查每层唯一键；不能先普通jsonb_in吞掉重复键，
	 * 再以规范化后的对象冒充已验证。转义后相同的键也由原生解析器拒绝。
	 */
	return DatumGetJsonbP(jsonb_from_text(raw, true));
}

static void
NeonDsgParse(text *raw, bool resize, NeonDsgIntent *intent)
{
	Jsonb	   *input = NeonDsgParseRaw(raw);
	JsonbContainer *root;
	JsonbContainer *key;
	JsonbContainer *instance;
	JsonbContainer *operation;
	uint64		target;

	MemSet(intent, 0, sizeof(*intent));
	root = &input->root;
	NeonDsgObject(root, resize ? 8 : 2);
	if (NeonDsgInteger(root, "protocol_version") != 1)
		ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("unsupported DSGA control protocol version")));
	key = NeonDsgChild(root, "key", 4);
	instance = NeonDsgChild(key, "instance", 3);
	operation = NeonDsgChild(key, "operation_id", 2);
	intent->vm_uid = NeonDsgId(instance, "vm_uid");
	intent->compute_instance_id = NeonDsgId(instance, "compute_instance_id");
	intent->key.boot_id = NeonDsgUint64(instance, "pg_boot_id");
	intent->key.issuer_epoch = NeonDsgUint64(key, "issuer_epoch");
	intent->key.revision = NeonDsgUint64(key, "revision");
	intent->key.operation_id_high = NeonDsgUint64(operation, "high");
	intent->key.operation_id_low = NeonDsgUint64(operation, "low");
	if (intent->key.boot_id == 0 || intent->key.issuer_epoch == 0 || intent->key.revision == 0 ||
		(intent->key.operation_id_high == 0 && intent->key.operation_id_low == 0))
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("DSGA control identity must be nonzero")));
	if (!resize)
		return;
	{
		JsonbValue *kind = NeonDsgField(root, "kind", jbvString);

		if (kind->val.string.len == 4 && memcmp(kind->val.string.val, "grow", 4) == 0)
			intent->kind = DSG_OP_GROW;
		else if (kind->val.string.len == 6 && memcmp(kind->val.string.val, "shrink", 6) == 0)
			intent->kind = DSG_OP_SHRINK;
		else
			ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("invalid DSGA resize kind")));
	}
	intent->request.boot_id = intent->key.boot_id;
	intent->request.issuer_epoch = intent->key.issuer_epoch;
	intent->request.revision = intent->key.revision;
	intent->request.operation_id_high = intent->key.operation_id_high;
	intent->request.operation_id_low = intent->key.operation_id_low;
	intent->request.expected_generation = NeonDsgUint64(root, "expected_capacity_generation");
	intent->target_vm_memory_bytes = NeonDsgUint64(root, "target_vm_memory_bytes");
	intent->deadline_unix_ms = NeonDsgUint64(root, "deadline_unix_ms");
	intent->budget_commitment_id = NeonDsgId(root, "budget_commitment_id");
	target = NeonDsgInteger(root, "target_buffers");
	if (target < 16 || target > INT_MAX / 2 || intent->request.expected_generation == 0 ||
		intent->target_vm_memory_bytes == 0 || intent->deadline_unix_ms == 0)
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("invalid DSGA resize range")));
	intent->request.target_buffers = (int) target;
	NeonDsgHashIntent(input, intent->request.context_hash);
}

static bool
NeonDsgIdentityBound(void)
{
	return dsg_vm_uid != NULL && *dsg_vm_uid &&
		dsg_compute_instance_id != NULL && *dsg_compute_instance_id;
}

static void
NeonDsgCheckEnabled(void)
{
	if (DSBCtrl == NULL)
		ereport(ERROR, (errcode(ERRCODE_OBJECT_NOT_IN_PREREQUISITE_STATE), errmsg("dynamic shared buffers are disabled")));
	if (!NeonDsgIdentityBound())
		ereport(ERROR, (errcode(ERRCODE_OBJECT_NOT_IN_PREREQUISITE_STATE), errmsg("DSGA managed instance identity is not configured")));
}

static void
NeonDsgAppendInstance(StringInfo output, const char *vm_uid,
					  const char *compute_instance_id, const uint64 *boot_id)
{
	appendStringInfoString(output, "{\"vm_uid\":");
	escape_json(output, vm_uid);
	appendStringInfoString(output, ",\"compute_instance_id\":");
	escape_json(output, compute_instance_id);
	if (boot_id != NULL)
		appendStringInfo(output, ",\"pg_boot_id\":\"" UINT64_FORMAT "\"}", *boot_id);
	else
		appendStringInfoString(output, ",\"pg_boot_id\":null}");
}

static void
NeonDsgAppendKey(StringInfo output, const DsgBufferRequestKey *key,
				 const char *vm_uid, const char *compute_instance_id)
{
	appendStringInfoString(output, "{\"instance\":");
	NeonDsgAppendInstance(output, vm_uid, compute_instance_id, &key->boot_id);
	appendStringInfo(output, ",\"issuer_epoch\":\"" UINT64_FORMAT "\",\"revision\":\"" UINT64_FORMAT
					 "\",\"operation_id\":{\"high\":\"" UINT64_FORMAT "\",\"low\":\"" UINT64_FORMAT "\"}}",
					 key->issuer_epoch, key->revision, key->operation_id_high, key->operation_id_low);
}

static const char *
NeonDsgEnumName(int value, const char *const *names, int count)
{
	if (value < 0 || value >= count)
		elog(ERROR, "invalid DSGA control state");
	return names[value];
}

static void
NeonDsgAppendSnapshot(StringInfo output, const DsgBufferSnapshot *snapshot)
{
	static const char *const phases[] = {"idle", "preparing", "committed", "denied"};
	static const char *const reasons[] = {"none", "timeout", "populate_failed", "interrupted", "cancelled"};
	static const char *const arrays[] = {"descriptors", "blocks", "iocv", "checkpoint"};
	static const char *const releases[] = {"none", "pending", "wait_checkpoint", "verified", "unsupported", "failed"};
	static const char *const memory[] = {"ok", "noop", "unsupported", "failed"};
	DsgBufferRequestKey key;
	int			i;
	char		context_hex[DSG_BUFFER_CONTEXT_BYTES * 2 + 1];
	const char *hex = "0123456789abcdef";
	const DsgBufferResizeRequest *request = &snapshot->operation.request;

	/*- IDLE也必须带完整当前实例；STALE请求的receipt身份不能代替observed身份。 */
	appendStringInfoString(output, "{\"instance\":");
	NeonDsgAppendInstance(output, dsg_vm_uid, dsg_compute_instance_id, &snapshot->boot_id);
	appendStringInfo(output, ",\"issuer_floor_epoch\":\"" UINT64_FORMAT
					 "\",\"capacity_generation\":\"" UINT64_FORMAT
					 "\",\"low_buffers\":%d,\"high_buffers\":%d,\"maximum_buffers\":%d,"
					 "\"admission_open\":%s,\"admission_revision\":\"" UINT64_FORMAT
					 "\",\"startup_shared_upper_bytes\":\"%zu\",\"operation\":",
					 snapshot->issuer_floor_epoch, snapshot->generation, snapshot->low, snapshot->high, snapshot->maximum,
					 snapshot->admission_open ? "true" : "false", snapshot->admission_revision, snapshot->startup_shared_upper_bytes);
	if (snapshot->operation.phase == DSG_OP_IDLE)
		appendStringInfoString(output, "null");
	else
	{
		key.boot_id = request->boot_id;
		key.issuer_epoch = request->issuer_epoch;
		key.revision = request->revision;
		key.operation_id_high = request->operation_id_high;
		key.operation_id_low = request->operation_id_low;
		appendStringInfoString(output, "{\"key\":");
		NeonDsgAppendKey(output, &key, dsg_vm_uid, dsg_compute_instance_id);
		for (i = 0; i < DSG_BUFFER_CONTEXT_BYTES; i++)
		{
			context_hex[2 * i] = hex[request->context_hash[i] >> 4];
			context_hex[2 * i + 1] = hex[request->context_hash[i] & 15];
		}
		context_hex[DSG_BUFFER_CONTEXT_BYTES * 2] = '\0';
		appendStringInfo(output,
						 ",\"kind\":\"%s\",\"phase\":\"%s\",\"reason\":\"%s\",\"target_buffers\":%d,"
						 "\"expected_capacity_generation\":\"" UINT64_FORMAT "\",\"context_sha256\":\"%s\","
						 "\"cancel_requested\":%s,\"cancel_closed\":%s,\"replayed\":%s}",
						 snapshot->operation.kind == DSG_OP_GROW ? "grow" : "shrink",
						 NeonDsgEnumName(snapshot->operation.phase, phases, lengthof(phases)),
						 NeonDsgEnumName(snapshot->operation.reason, reasons, lengthof(reasons)), request->target_buffers,
						 request->expected_generation, context_hex, snapshot->cancel_requested ? "true" : "false",
						 snapshot->cancel_closed ? "true" : "false", snapshot->replayed ? "true" : "false");
	}
	appendStringInfoString(output, ",\"arrays\":[");
	for (i = 0; i < DSG_BUFFER_ARRAY_COUNT; i++)
	{
		const DsgMemoryResult *result = &snapshot->release_result[i];
		bool		has_result = snapshot->release_state[i] == DSG_RELEASE_VERIFIED ||
			snapshot->release_state[i] == DSG_RELEASE_UNSUPPORTED ||
			snapshot->release_state[i] == DSG_RELEASE_FAILED;

		if (i != 0)
			appendStringInfoChar(output, ',');
		appendStringInfo(output,
						 "{\"array\":\"%s\",\"committed_upper_bytes\":\"%zu\",\"allocation_unknown\":%s,"
						 "\"release_state\":\"%s\",\"release_key\":",
						 arrays[i], snapshot->committed_upper_bytes[i], snapshot->allocation_unknown[i] ? "true" : "false",
						 NeonDsgEnumName(snapshot->release_state[i], releases, lengthof(releases)));
		if (snapshot->release_state[i] == DSG_RELEASE_NONE)
			appendStringInfoString(output, "null");
		else
		{
			key.boot_id = snapshot->boot_id;
			key.issuer_epoch = snapshot->release_issuer_epoch[i];
			key.revision = snapshot->release_revision[i];
			key.operation_id_high = snapshot->release_operation_id_high[i];
			key.operation_id_low = snapshot->release_operation_id_low[i];
			NeonDsgAppendKey(output, &key, dsg_vm_uid, dsg_compute_instance_id);
		}
		appendStringInfo(output,
						 ",\"release_capacity_generation\":\"" UINT64_FORMAT "\",\"provider\":{\"has_result\":%s,\"status\":\"%s\","
						 "\"system_errno\":%d,\"requested_offset\":\"%zu\",\"requested_bytes\":\"%zu\","
						 "\"affected_offset\":\"%zu\",\"affected_bytes\":\"%zu\",\"physical_release_verified\":%s,"
						 "\"allocation_may_be_partial\":%s,\"release_may_be_partial\":%s}}",
						 snapshot->release_generation[i], has_result ? "true" : "false",
						 has_result ? NeonDsgEnumName(result->status, memory, lengthof(memory)) : "unknown",
						 result->system_errno, result->requested_offset, result->requested_bytes,
						 result->affected_offset, result->affected_bytes, result->physical_release_verified ? "true" : "false",
						 result->allocation_may_be_partial ? "true" : "false", result->release_may_be_partial ? "true" : "false");
	}
	appendStringInfoString(output, "]}");
}

static Datum
NeonDsgRespond(const NeonDsgIntent *intent, const DsgBufferControlResponse *response)
{
	static const char *const dispositions[] = {"accepted", "replayed", "busy", "stale", "conflict",
	"result_expired", "not_found", "cancel_requested", "too_late", "rejected"};
	StringInfoData output;

	initStringInfo(&output);
	appendStringInfoString(&output, "{\"protocol_version\":1,\"vm_shrink_permit\":false,\"receipt\":{\"key\":");
	NeonDsgAppendKey(&output, &response->receipt.key, intent->vm_uid, intent->compute_instance_id);
	appendStringInfo(&output, ",\"disposition\":\"%s\",\"has_result\":%s,\"result\":",
					 NeonDsgEnumName(response->receipt.disposition, dispositions, lengthof(dispositions)),
					 response->receipt.has_result ? "true" : "false");
	if (response->receipt.has_result)
		NeonDsgAppendSnapshot(&output, &response->receipt.result);
	else
		appendStringInfoString(&output, "null");
	appendStringInfoString(&output, "},\"observed\":");
	NeonDsgAppendSnapshot(&output, &response->observed);
	appendStringInfoChar(&output, '}');
	return NeonDsgJson(&output);
}

static int
NeonDsgRemainingWait(uint64 deadline)
{
	int64		now_ms;
	uint64		remaining;

	/*- PG纪元2000转Unix纪元；先检查有符号加法，剩余时间不做无符号下溢。 */
	if (pg_add_s64_overflow(GetCurrentTimestamp() / 1000, INT64CONST(946684800000), &now_ms) || now_ms < 0)
		ereport(ERROR, (errcode(ERRCODE_DATETIME_VALUE_OUT_OF_RANGE), errmsg("DSGA clock is outside supported Unix range")));
	remaining = deadline > (uint64) now_ms ? deadline - (uint64) now_ms : 0;
	return (int) Min(remaining, UINT64CONST(60000));
}

static Datum
NeonDsgExecute(text *raw, int action)
{
	NeonDsgIntent intent;
	DsgBufferControlResponse response;

	NeonDsgCheckEnabled();
	NeonDsgParse(raw, action == 0, &intent);
	MemSet(&response, 0, sizeof(response));
	if (strcmp(intent.vm_uid, dsg_vm_uid) != 0 ||
		strcmp(intent.compute_instance_id, dsg_compute_instance_id) != 0)
	{
		response.receipt.key = intent.key;
		response.receipt.disposition = DSG_RECEIPT_STALE;
		DsgBufferGetSnapshot(&response.observed);
	}
	else if (action == 0)
	{
		/*-
		 * 内核先查完整历史，再应用剩余等待；过期旧意图仍能幂等查询。
		 * budget_commitment_id只是绑定引用，认证调用方仍必须先核验承诺
		 * 和当前issuer。本入口不从RSS/PG逻辑提交推导任何VM执行许可。
		 */
		DsgBufferControlResize(&intent.request, intent.kind, NeonDsgRemainingWait(intent.deadline_unix_ms), &response);
	}
	else if (action == 1)
		DsgBufferControlStatus(&intent.key, &response);
	else
		DsgBufferControlCancel(&intent.key, &response);
	return NeonDsgRespond(&intent, &response);
}

/*-
 * 独立接管不改变容量/预算，不接收调用者自报的“已完成”。完整实例和代次从
 * 原始严格对象读取；C层只认证管理员会话，真实issuer租约仍由上层认证执行链
 * 核验。PENDING不能当接管成功，旧Provider未退出时不发任何PG/VM完成许可。
 */
static Datum
NeonDsgFence(text *raw)
{
	Jsonb	   *input = NeonDsgParseRaw(raw);
	JsonbContainer *root = &input->root;
	JsonbContainer *instance;
	char	   *vm_uid;
	char	   *compute_instance_id;
	uint64		boot;
	uint64		epoch;
	DsgBufferIssuerFenceResponse response;
	StringInfoData output;
	static const char *const dispositions[] = {"fenced", "pending", "stale"};

	NeonDsgCheckEnabled();
	NeonDsgObject(root, 3);
	if (NeonDsgInteger(root, "protocol_version") != 1)
		ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("unsupported DSGA control protocol version")));
	instance = NeonDsgChild(root, "instance", 3);
	vm_uid = NeonDsgId(instance, "vm_uid");
	compute_instance_id = NeonDsgId(instance, "compute_instance_id");
	boot = NeonDsgUint64(instance, "pg_boot_id");
	epoch = NeonDsgUint64(root, "issuer_epoch");
	if (boot == 0 || epoch == 0)
		ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("DSGA issuer fence identity must be nonzero")));
	if (strcmp(vm_uid, dsg_vm_uid) != 0 || strcmp(compute_instance_id, dsg_compute_instance_id) != 0)
	{
		MemSet(&response, 0, sizeof(response));
		response.disposition = DSG_ISSUER_STALE;
		DsgBufferGetSnapshot(&response.observed);
	}
	else
		DsgBufferFenceIssuer(boot, epoch, &response);
	initStringInfo(&output);
	appendStringInfoString(&output, "{\"protocol_version\":1,\"vm_shrink_permit\":false,\"requested_instance\":");
	NeonDsgAppendInstance(&output, vm_uid, compute_instance_id, &boot);
	appendStringInfo(&output, ",\"requested_epoch\":\"" UINT64_FORMAT "\",\"disposition\":\"%s\",\"observed\":",
					 epoch, NeonDsgEnumName(response.disposition, dispositions, lengthof(dispositions)));
	NeonDsgAppendSnapshot(&output, &response.observed);
	appendStringInfoChar(&output, '}');
	return NeonDsgJson(&output);
}

#endif

Datum
neon_dsg_capabilities(PG_FUNCTION_ARGS)
{
	StringInfoData output;

	NeonDsgCheckAdministrator();
	initStringInfo(&output);
	appendStringInfoString(&output, "{\"protocol_version\":1,\"production_ready\":false,\"vm_shrink_permit\":false,");
#ifdef NEON_DSG_CONTROL_SUPPORTED
	appendStringInfo(&output,
					 "\"supported\":true,\"buffer_api_version\":%d,\"enabled\":%s,\"instance_bound\":%s,"
					 "\"history_capacity\":%d,\"provider_can_release\":%s,\"instance\":{\"vm_uid\":",
					 NEON_DSG_BUFFER_API_VERSION, DSBCtrl != NULL ? "true" : "false", NeonDsgIdentityBound() ? "true" : "false",
					 DSG_BUFFER_HISTORY_SIZE, DsgRegionCanRelease() ? "true" : "false");
	escape_json(&output, dsg_vm_uid == NULL ? "" : dsg_vm_uid);
	appendStringInfoString(&output, ",\"compute_instance_id\":");
	escape_json(&output, dsg_compute_instance_id == NULL ? "" : dsg_compute_instance_id);
	if (DSBCtrl != NULL)
		appendStringInfo(&output, ",\"pg_boot_id\":\"" UINT64_FORMAT "\"}}", DSBCtrl->boot_id);
	else
		appendStringInfoString(&output, ",\"pg_boot_id\":null}}");
#else
	appendStringInfoString(&output, "\"supported\":false,\"buffer_api_version\":0,\"enabled\":false,\"instance_bound\":false}");
#endif
	PG_RETURN_DATUM(NeonDsgJson(&output));
}

Datum
neon_dsg_shared_cost(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#if defined(NEON_DSG_CONTROL_SUPPORTED) && defined(NEON_DSG_SHARED_COST_API_VERSION) && NEON_DSG_SHARED_COST_API_VERSION == 1
	{
		DsgBufferSharedCost cost;
		StringInfoData output;

		NeonDsgCheckEnabled();
		DsgBufferGetSharedCost(&cost);
		initStringInfo(&output);
		/*- 所有64位成本都用规范字符串；返回PG成本事实而非完整Guest预算/许可。 */
		appendStringInfo(&output,
			"{\"protocol_version\":1,\"vm_shrink_permit\":false,"
			"\"startup_main_upper_bytes\":\"%zu\",\"startup_arrays_upper_bytes\":\"%zu\","
			"\"current_main_and_arrays_upper_bytes\":\"%zu\",\"observed\":",
			cost.startup_main_upper_bytes, cost.startup_arrays_upper_bytes,
			cost.current_main_and_arrays_upper_bytes);
		NeonDsgAppendSnapshot(&output, &cost.observed);
		appendStringInfoChar(&output, '}');
		PG_RETURN_DATUM(NeonDsgJson(&output));
	}
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

/*-
 * 只读固定大小Native登记，同一次锁内包含实例代次和门禁。不接受调用者自报
 * 的idle/PID/完成，也不把前台空闲提升成完整Quiesce或VM缩容许可。
 * 与其他内部入口相同，SQL ACL之外仍核验当前及session身份，弱SU不能借函数绕过。
 */
Datum
neon_dsg_work_state(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#if defined(NEON_DSG_CONTROL_SUPPORTED) && defined(NEON_DSG_WORK_API_VERSION) && NEON_DSG_WORK_API_VERSION == 2
	{
		DsgWorkSnapshot s;
		StringInfoData output;

		NeonDsgCheckEnabled();
		DsgWorkGetSnapshot(&s);
		initStringInfo(&output);
		appendStringInfo(&output,
			"{\"protocol_version\":1,\"work_api_version\":2,\"vm_shrink_permit\":false,"
			"\"client_processes\":%u,\"startup_clients\":%u,\"idle_clients\":%u,"
			"\"frontend_reads\":%u,\"frontend_executes\":%u,\"bounded_inputs\":%u,"
			"\"retained_transactions\":%u,\"background_processes\":%u,\"auxiliary_processes\":%u,"
			"\"observer_excluded\":%s,\"foreground_idle\":%s,\"observed\":",
			s.client_processes, s.startup_clients, s.idle_clients, s.frontend_reads,
			s.frontend_executes, s.bounded_inputs, s.retained_transactions,
			s.background_processes, s.auxiliary_processes,
			s.observer_excluded ? "true" : "false", s.foreground_idle ? "true" : "false");
		NeonDsgAppendSnapshot(&output, &s.observed);
		appendStringInfoChar(&output, '}');
		PG_RETURN_DATUM(NeonDsgJson(&output));
	}
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

Datum
neon_dsg_memory_status(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#ifdef NEON_DSG_CONTROL_SUPPORTED
	{
		DsgBufferSnapshot snapshot;
		StringInfoData output;

		NeonDsgCheckEnabled();
		DsgBufferGetSnapshot(&snapshot);
		initStringInfo(&output);
		appendStringInfoString(&output, "{\"protocol_version\":1,\"vm_shrink_permit\":false,\"observed\":");
		NeonDsgAppendSnapshot(&output, &snapshot);
		appendStringInfoChar(&output, '}');
		PG_RETURN_DATUM(NeonDsgJson(&output));
	}
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

Datum
neon_dsg_request_resize(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#ifdef NEON_DSG_CONTROL_SUPPORTED
	PG_RETURN_DATUM(NeonDsgExecute(PG_GETARG_TEXT_PP(0), 0));
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

Datum
neon_dsg_resize_status(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#ifdef NEON_DSG_CONTROL_SUPPORTED
	PG_RETURN_DATUM(NeonDsgExecute(PG_GETARG_TEXT_PP(0), 1));
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

Datum
neon_dsg_cancel_resize(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#ifdef NEON_DSG_CONTROL_SUPPORTED
	PG_RETURN_DATUM(NeonDsgExecute(PG_GETARG_TEXT_PP(0), 2));
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

Datum
neon_dsg_fence_issuer(PG_FUNCTION_ARGS)
{
	NeonDsgCheckAdministrator();
#ifdef NEON_DSG_CONTROL_SUPPORTED
	PG_RETURN_DATUM(NeonDsgFence(PG_GETARG_TEXT_PP(0)));
#else
	NeonDsgUnsupported();
	PG_RETURN_NULL();
#endif
}

/*-
 * 正式准入只接受严格完整原意图，预算字段是已认证外层的引用，不是C层自证预算。
 * 不能用创建只读JWT调用；身份/issuer授权由HTTP与管理连接共同约束。内核还会
 * 原子检查已建立的issuer floor、boot、容量与准入revision及整个意图摘要。
 * 不把开门/关门当作已有工作排空，更不发放物理回收或VM执行许可。
 */
static Datum
NeonDsgAdmission(text *raw, bool query)
{
	NeonDsgCheckAdministrator();
#ifdef NEON_DSG_ADMISSION_SUPPORTED
	{
		Jsonb	   *input = NeonDsgParseRaw(raw);
		JsonbContainer *root = &input->root;
		JsonbContainer *instance;
		char	   *vm_uid;
		char	   *compute_instance_id;
		DsgBufferAdmissionRequest request;
		DsgBufferAdmissionResponse response;
		StringInfoData output;
		int			i;
		static const char *const dispositions[] = {"applied", "replayed", "busy", "stale", "conflict", "not_found", "result_expired"};

		NeonDsgCheckEnabled();
		NeonDsgObject(root, 7);
		if (NeonDsgInteger(root, "protocol_version") != 1)
			ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("unsupported DSGA admission protocol version")));
		instance = NeonDsgChild(root, "instance", 3);
		vm_uid = NeonDsgId(instance, "vm_uid");
		compute_instance_id = NeonDsgId(instance, "compute_instance_id");
		MemSet(&request, 0, sizeof(request));
		request.boot_id = NeonDsgUint64(instance, "pg_boot_id");
		request.issuer_epoch = NeonDsgUint64(root, "issuer_epoch");
		request.revision = NeonDsgUint64(root, "revision");
		request.expected_generation = NeonDsgUint64(root, "expected_capacity_generation");
		request.open = NeonDsgField(root, "open", jbvBool)->val.boolean;
		(void) NeonDsgId(root, "budget_commitment_id");
		if (request.boot_id == 0 || request.issuer_epoch == 0 || request.revision == 0 || request.expected_generation == 0)
			ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE), errmsg("DSGA admission identity must be nonzero")));
		NeonDsgHashIntent(input, request.context_sha256);
		if (strcmp(vm_uid, dsg_vm_uid) != 0 || strcmp(compute_instance_id, dsg_compute_instance_id) != 0)
		{
			MemSet(&response, 0, sizeof(response));
			response.disposition = DSG_ADMISSION_STALE;
			DsgBufferGetSnapshot(&response.observed);
		}
		else if (!query)
			DsgBufferControlAdmission(&request, &response);
		else
		{
#if defined(NEON_DSG_ADMISSION_STATUS_API_VERSION) && NEON_DSG_ADMISSION_STATUS_API_VERSION == 1
			DsgBufferAdmissionStatus(&request, &response);
#else
			ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("this Neon build does not support DSGA admission status protocol 1")));
#endif
		}
		initStringInfo(&output);
		appendStringInfoString(&output, "{\"protocol_version\":1,\"vm_shrink_permit\":false,\"request\":");
		appendStringInfoString(&output, JsonbToCString(NULL, root, VARSIZE(input)));
		appendStringInfo(&output, ",\"disposition\":\"%s\",\"context_sha256\":\"",
						 NeonDsgEnumName(response.disposition, dispositions, lengthof(dispositions)));
		for (i = 0; i < DSG_BUFFER_CONTEXT_BYTES; i++)
			appendStringInfo(&output, "%02x", request.context_sha256[i]);
		appendStringInfoString(&output, "\",\"observed\":");
		NeonDsgAppendSnapshot(&output, &response.observed);
		appendStringInfoChar(&output, '}');
		PG_RETURN_DATUM(NeonDsgJson(&output));
	}
#else
	ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("this Neon build does not support DSGA admission protocol 1")));
	PG_RETURN_NULL();
#endif
}

/*- 两个独立SQL入口固定行为，不从请求字段接受“只读/执行”开关。 */
Datum
neon_dsg_set_admission(PG_FUNCTION_ARGS)
{
	return NeonDsgAdmission(PG_GETARG_TEXT_PP(0), false);
}

Datum
neon_dsg_admission_status(PG_FUNCTION_ARGS)
{
	return NeonDsgAdmission(PG_GETARG_TEXT_PP(0), true);
}
