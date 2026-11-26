import unittest
from desktop_gpu_stall_trace import parse_commands,correlate

class CorrelationTest(unittest.TestCase):
    def row(self):
        row=[0]*20;row[0]=7;row[3]=21000;row[6]=20000
        row[16:20]=[1,3,3000,5]
        return {'run':{'samples':[row]}}
    def records(self,fenced):
        lines=['9@10.000:virtio_gpu_cmd_ctx_submit ctx 1, size 100']
        if fenced:lines.append('9@10.001:virtio_gpu_fence_ctrl fence 0x1, type 0x207')
        lines.append('9@10.002:virtio_gpu_cmd_set_scanout id 0, res 7')
        if fenced:lines.append('9@10.003:virtio_gpu_fence_ctrl fence 0x2, type 0x103')
        lines+=['9@10.004:virtio_gpu_cmd_res_flush res 7',
                '9@10.005:virtio_gpu_fence_ctrl fence 0x3, type 0x104',
                '9@10.020:virtio_gpu_fence_resp fence 0x3']
        if fenced:lines+=['9@10.021:virtio_gpu_fence_resp fence 0x1','9@10.022:virtio_gpu_fence_resp fence 0x2']
        return lines
    def test_fenced_commands_with_reversed_completions(self):
        commands,_=parse_commands(self.records(True));frame=correlate(commands,self.row())[0]
        self.assertAlmostEqual(frame['host_frame_ms'],20)
        self.assertAlmostEqual(frame['draw']['execution_ms'],1)
        self.assertAlmostEqual(frame['queue_or_wakeup_residual_ms'],0)
    def test_final_fence_only_keeps_dispatch_and_wait_separate(self):
        commands,_=parse_commands(self.records(False));frame=correlate(commands,self.row())[0]
        self.assertFalse(frame['draw']['fenced']);self.assertAlmostEqual(frame['host_draw_to_scanout_ms'],2)
        self.assertAlmostEqual(frame['flush']['fence_ms'],15)
    def test_incomplete_trace_does_not_invent_completion(self):
        commands,_=parse_commands(self.records(False)[:-1]);self.assertEqual(correlate(commands,self.row()),[])

if __name__=='__main__':unittest.main()
