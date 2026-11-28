#!/usr/bin/env python3
"""Selected-file bounds and actual guest background-reader state machine."""
import subprocess,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class SelectedFiles(unittest.TestCase):
    def test_guest_reader_and_revocation(self):
        binary=ROOT/'build/maps-files-store-test'
        subprocess.run(['clang','-O1','-g','-fsanitize=address,undefined',str(ROOT/'tests/maps_files_store_test.c'),str(ROOT/'experiments/comaps-native/hash.c'),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)
if __name__=='__main__':unittest.main()
