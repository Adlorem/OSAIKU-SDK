"""Exercise the real authoring function and independent SDK checker; no UI."""
import copy, hashlib, importlib.util, struct, subprocess, sys, tempfile, unittest
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

CHECKER=Path(sys.argv.pop(1)).resolve()
spec=importlib.util.spec_from_file_location('author',Path(__file__).with_name('package.py'))
author=importlib.util.module_from_spec(spec);spec.loader.exec_module(author)

class EmbeddedKey(unittest.TestCase):
    def setUp(self):
        temp=tempfile.TemporaryDirectory(prefix='embedded-driver-')
        self.addCleanup(temp.cleanup);self.root=Path(temp.name)
        self.key=Ed25519PrivateKey.from_private_bytes(bytes([0x42])*32)
        elf=bytearray(64);elf[:7]=b'\x7fELF\x02\x01\x01'
        struct.pack_into('<HHI',elf,16,2,62,1);struct.pack_into('<H',elf,52,64)
        (self.root/'driver.elf').write_bytes(elf)
        self.config={'package':dict(id='example.keyed',name='Keyed',version='1',release=1,
            epoch=2,executable='driver.elf',memory_bytes=262144),
            'compatibility':dict(abi_major=1),
            'driver':dict(classes=1,cpu_permille=50,queue=8,isolation='mediated',reset='function',include_public_key=True),
            'matches':[dict(bus='usb',**{'class':3})]}
    def check(self,b,ok=True):
        p=self.root/'driver.osadrv';p.write_bytes(b)
        r=subprocess.run([str(CHECKER),'inspect-driver',str(p)],capture_output=True,text=True)
        self.assertEqual(r.returncode==0,ok,r.stdout+r.stderr)
    def test_key_is_part_of_signed_payload_and_legacy_is_unchanged(self):
        b=author.pack(self.config,self.root,self.key,True)
        self.assertEqual(struct.unpack_from('<I',b,200)[0],3)
        self.assertEqual(b[672:704],self.key.public_key().public_bytes_raw())
        self.assertEqual(b[32:64],hashlib.sha256(b[672:704]).digest())
        self.assertEqual(b[64:96],hashlib.sha256(b[192:]).digest())
        self.key.public_key().verify(b[128:192],b'OSAIKU/driver-package/v1\0'+b[:128])
        self.check(b)
        del self.config['driver']['include_public_key']
        legacy=author.pack(self.config,self.root,self.key,True)
        self.assertEqual(struct.unpack_from('<I',legacy,200)[0],1)
        self.assertEqual(legacy[666:704],bytes(38));self.check(legacy)
    def test_gpu_two_elf_package_keeps_gpu_contract_and_revision_bounds(self):
        # Keep this SDK test runnable without the OS checkout or driver sources.
        config=copy.deepcopy(self.config)
        del config['package']['executable']
        config['driver'].update(classes=64,isolation='iommu',reset='cold')
        config['compatibility']['protocols']=[dict(id=i,major=1) for i in (11,12,13)]
        config['matches']=[dict(bus='pci',vendor=0x8086,device=i,**{'class':3},
            revision_min=0,revision_max=5) for i in (0x9bc2,0x9bc4,0x9bc5,0x9bc6,0x9bc8,
                0x9be6,0x9bf6,0x9b41,0x9bca,0x9bcc)]
        config['gpu']=dict(family='intel-cometlake',backend='driver.elf',renderer='driver.elf',
            platform_abi=dict(major=1),renderer_abi=dict(major=1),client_abi=dict(major=1),
            backend_budget=dict(memory=262144,dma=4096,stack=16384,threads=1,instances=1),
            renderer_budget=dict(memory=262144,stack=16384,threads=1,instances=1),
            dma_address_bits=39,gpu_address_bits=48,memory_model='unified')
        b=author.pack(config,self.root,self.key,True);self.check(b)
        self.assertEqual(struct.unpack_from('<I',b,200)[0],3)
        self.assertEqual(struct.unpack_from('<H',b,192+62)[0],2)
        self.assertEqual(struct.unpack_from('<H',b,192+470)[0],10)
        self.assertEqual(b[672:704],self.key.public_key().public_bytes_raw())
    def test_nonboolean_opt_in_is_refused(self):
        for value in ['true',1,None]:
            self.config['driver']['include_public_key']=value
            with self.assertRaises(ValueError): author.pack(self.config,self.root,self.key,True)

if __name__=='__main__':unittest.main()
