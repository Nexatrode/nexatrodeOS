#include <linux/qrtr.h>
#include <linux/unaligned.h>

#define CREATE_TRACE_POINTS
#include "sunfish_qrtr_trace.h"

static void sunfish_qrtr_observe(bool tx, const u8 *data, size_t len)
{
	u32 version, type, sn, sp, dn, dp, size;
	u32 control = U32_MAX, key_node = U32_MAX, key_port = U32_MAX;
	u32 kind = U32_MAX, txn = U32_MAX, msg = U32_MAX;
	size_t header;
	bool confirm;
	const u8 *body;

	if (!trace_header_enabled() || len < 16)
		return;

	version = data[0];
	if (version == 1 && len >= 32) {
		header = 32;
		type = get_unaligned_le32(data + 4);
		sn = get_unaligned_le32(data + 8);
		sp = get_unaligned_le32(data + 12);
		confirm = !!get_unaligned_le32(data + 16);
		size = get_unaligned_le32(data + 20);
		dn = get_unaligned_le32(data + 24);
		dp = get_unaligned_le32(data + 28);
	} else if (version == 3) {
		header = 16 + data[3];
		type = data[1];
		confirm = data[2] & 1;
		size = get_unaligned_le32(data + 4);
		sn = get_unaligned_le16(data + 8);
		sp = get_unaligned_le16(data + 10);
		dn = get_unaligned_le16(data + 12);
		dp = get_unaligned_le16(data + 14);
	} else {
		return;
	}
	if (header > len || size > len - header ||
	    len - header != ALIGN((size_t)size, 4))
		return;
	if (!((sn == 0 && (dn == 1 || dn == U32_MAX || dn == 0xffff)) ||
	      (sn == 1 && (dn == 0 || dn == U32_MAX || dn == 0xffff))))
		return;

	body = data + header;
	if (type != QRTR_TYPE_DATA && size >= 4) {
		control = get_unaligned_le32(body);
		if (size >= 12 && (control == QRTR_TYPE_DEL_CLIENT ||
				   control == QRTR_TYPE_RESUME_TX)) {
			key_node = get_unaligned_le32(body + 4);
			key_port = get_unaligned_le32(body + 8);
		}
	} else if (type == QRTR_TYPE_DATA && (len == 44 || len == 48) &&
		   size >= 7 && (body[0] == 0 || body[0] == 2 || body[0] == 4) &&
		   get_unaligned_le16(body + 5) == size - 7) {
		kind = body[0];
		txn = get_unaligned_le16(body + 1);
		msg = get_unaligned_le16(body + 3);
	}

	trace_header(&(struct sunfish_qrtr_header) {
		.tx = tx, .version = version, .type = type,
		.src_node = sn, .src_port = sp, .dst_node = dn, .dst_port = dp,
		.confirm = confirm, .length = len, .control = control,
		.key_node = key_node, .key_port = key_port,
		.qmi_kind = kind, .qmi_txn = txn, .qmi_msg = msg,
	});
}
