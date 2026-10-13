import hashlib
import runpy
from pathlib import Path
import tempfile
import unittest

verify = runpy.run_path(str(Path(__file__).with_name('final_identity.py')))['verify_final_plugin']


class FinalIdentity(unittest.TestCase):
    def test_post_fixup_mutation_rejects_prefixed_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            plugin = Path(directory) / 'qualified.so'
            plugin.write_bytes(b'build-output')
            expected = hashlib.sha256(plugin.read_bytes()).hexdigest()
            self.assertEqual(verify(plugin, expected), expected)
            plugin.write_bytes(b'installed-stripped-rpath-output')
            with self.assertRaisesRegex(ValueError, 'FINAL installed'):
                verify(plugin, expected)
            final = hashlib.sha256(plugin.read_bytes()).hexdigest()
            self.assertEqual(verify(plugin, final), final)


if __name__ == '__main__':
    unittest.main()
