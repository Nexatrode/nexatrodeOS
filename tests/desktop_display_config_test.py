import copy
import unittest
from desktop_display_config import configure_display

class DisplayTransportTest(unittest.TestCase):
    def setUp(self):
        self.config={'Display':[{'Hardware':'virtio-gpu-gl-device','DynamicResolution':False,'NativeResolution':False}],
                     'Input':{'UsbBusSupport':'Disabled'},'Sharing':{'ClipboardSharing':False,'DirectoryShareMode':'None'}}
    def test_display_only_agent_preserves_native_input_and_no_sharing(self):
        for flag in ('display_agent_probe','desktop_resize'):
            config=copy.deepcopy(self.config);arguments=['-device virtio-tablet-device','-device virtio-keyboard-device']
            configure_display(config,arguments,{flag:True})
            self.assertTrue(config['Display'][0]['DynamicResolution'])
            self.assertTrue(config['Display'][0]['NativeResolution'])
            self.assertIn('-spice agent-mouse=off',arguments)
            self.assertEqual(arguments[:2],['-device virtio-tablet-device','-device virtio-keyboard-device'])
            self.assertEqual(config['Input'],self.config['Input'])
            self.assertEqual(config['Sharing'],self.config['Sharing'])
    def test_no_agent_keeps_fixed_display_configuration(self):
        config=copy.deepcopy(self.config);arguments=[]
        configure_display(config,arguments,{'virgl':True})
        self.assertEqual(config,self.config);self.assertEqual(arguments,[])

if __name__=='__main__':unittest.main()
