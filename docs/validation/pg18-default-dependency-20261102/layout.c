#include "postgres.h"
#include "catalog/pg_control.h"
#include "access/xlog_internal.h"
#include "access/xlogrecord.h"
#include <stddef.h>
#include <unistd.h>
int main(void){const unsigned long v[]={PG_VERSION_NUM, PG_CONTROL_VERSION, XLOG_PAGE_MAGIC, sizeof(ControlFileData), offsetof(ControlFileData,checkPointCopy), sizeof(CheckPoint), offsetof(CheckPoint,wal_level), sizeof(XLogRecord), sizeof(XLogLongPageHeaderData)};return write(1,v,sizeof(v))!=sizeof(v);}
