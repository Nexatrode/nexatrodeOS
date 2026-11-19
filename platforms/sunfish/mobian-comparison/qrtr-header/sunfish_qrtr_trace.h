#undef TRACE_SYSTEM
#define TRACE_SYSTEM sunfish_qrtr

#if !defined(SUNFISH_QRTR_TRACE_H) || defined(TRACE_HEADER_MULTI_READ)
#define SUNFISH_QRTR_TRACE_H

#include <linux/tracepoint.h>

#ifndef SUNFISH_QRTR_HEADER_FIELDS
#define SUNFISH_QRTR_HEADER_FIELDS
struct sunfish_qrtr_header {
	bool tx, confirm;
	u32 version, type, src_node, src_port, dst_node, dst_port, length;
	u32 control, key_node, key_port, qmi_kind, qmi_txn, qmi_msg;
};
#endif

TRACE_EVENT(header,
	TP_PROTO(const struct sunfish_qrtr_header *h),
	TP_ARGS(h),
	TP_STRUCT__entry(
		__field(bool, tx)
		__field(u32, version)
		__field(u32, type)
		__field(u32, src_node)
		__field(u32, src_port)
		__field(u32, dst_node)
		__field(u32, dst_port)
		__field(bool, confirm)
		__field(u32, length)
		__field(u32, control)
		__field(u32, key_node)
		__field(u32, key_port)
		__field(u32, qmi_kind)
		__field(u32, qmi_txn)
		__field(u32, qmi_msg)
	),
	TP_fast_assign(
		__entry->tx = h->tx;
		__entry->version = h->version;
		__entry->type = h->type;
		__entry->src_node = h->src_node;
		__entry->src_port = h->src_port;
		__entry->dst_node = h->dst_node;
		__entry->dst_port = h->dst_port;
		__entry->confirm = h->confirm;
		__entry->length = h->length;
		__entry->control = h->control;
		__entry->key_node = h->key_node;
		__entry->key_port = h->key_port;
		__entry->qmi_kind = h->qmi_kind;
		__entry->qmi_txn = h->qmi_txn;
		__entry->qmi_msg = h->qmi_msg;
	),
	TP_printk("%s v=%u type=%u src=%u:%#x dst=%u:%#x confirm=%u len=%u ctrl=%#x key=%u:%#x qmi=%#x/%#x/%#x",
		  __entry->tx ? "TX" : "RX", __entry->version, __entry->type,
		  __entry->src_node, __entry->src_port, __entry->dst_node,
		  __entry->dst_port, __entry->confirm, __entry->length,
		  __entry->control, __entry->key_node, __entry->key_port,
		  __entry->qmi_kind, __entry->qmi_txn, __entry->qmi_msg)
);

#endif

#undef TRACE_INCLUDE_PATH
#define TRACE_INCLUDE_PATH .
#undef TRACE_INCLUDE_FILE
#define TRACE_INCLUDE_FILE sunfish_qrtr_trace
#include <trace/define_trace.h>
