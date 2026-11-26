import unittest
from desktop_boot_stress import registers

class RegistersTest(unittest.TestCase):
    def test_explicit_cpu_argument(self):
        class Q:
            def send(self,command,args):
                self.assertion=(command,args)
                return {'return':'CPU#2\n PC=ffffff8040010f08 X21=1234 SP=ffff'}
        q=Q();r=registers(q,2)
        self.assertEqual(q.assertion,('human-monitor-command',{'command-line':'info registers','cpu-index':2}))
        self.assertEqual(r['values']['X21'],0x1234)

    def test_repeated_cpu_zero_is_rejected(self):
        class Q:
            def send(self,*args):return {'return':'CPU#0\n PC=ffffff8040010f08'}
        with self.assertRaises(AssertionError):registers(Q(),3)

if __name__=='__main__':unittest.main()
