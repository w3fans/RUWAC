"""Fail-closed recognition tests; synthetic OCR is never remote-login evidence."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from PIL import Image, ImageDraw

HELPER = Path(__file__).resolve().parents[3] / 'scripts/gdm-recognize.py'


class RecognitionGuards(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.image = self.root / 'frame.png'
        Image.new('RGB', (1280, 720), '#222222').save(self.image)

    def tearDown(self):
        self.temp.cleanup()

    def run_helper(self, tool=None):
        env = dict(os.environ)
        if tool is not None:
            env['RUWAC_GDM_OCR_TOOL'] = str(tool)
        else:
            env.pop('RUWAC_GDM_OCR_TOOL', None)
        result = subprocess.run(['python3', str(HELPER), str(self.image)],
                                env=env, capture_output=True, text=True, timeout=7)
        observation = json.loads(result.stdout)
        self.assertNotIn('rect', observation)
        return observation['kind'], result.returncode

    def fake_ocr(self, confidence=99, malformed=False, branding_confidence=99):
        tool = self.root / 'fake-ocr'
        body = "print('malformed TSV')" if malformed else f"""
from pathlib import Path
import sys
label = 'fedora' if Path(sys.argv[1]).name in ['layout.png', 'branding.png'] else 'Password'
confidence = {branding_confidence} if label == 'fedora' else {confidence}
print('left\\ttop\\twidth\\theight\\tconf\\ttext')
print(f'30\\t30\\t150\\t30\\t{{confidence}}\\t{{label}}')
"""
        tool.write_text('#!/usr/bin/python3\n' + body + '\n')
        tool.chmod(0o700)
        return tool

    def focus_boxes(self, count):
        image = Image.open(self.image)
        draw = ImageDraw.Draw(image)
        for index in range(count):
            y = 200 + index * 100
            draw.rectangle((100, y, 400, y + 40), outline=(150, 180, 205))
        image.save(self.image)

    def test_blank_frame_never_authorizes_input(self):
        self.assertEqual(self.run_helper(), ('unknown', 0))

    def test_missing_ocr_is_unavailable(self):
        self.assertEqual(self.run_helper(self.root / 'absent'), ('unavailable', 1))

    def test_unsupported_dimensions_are_unavailable(self):
        for size in [(100, 100), (5000, 200)]:
            with self.subTest(size=size):
                Image.new('RGB', size).save(self.image)
                self.assertEqual(self.run_helper(), ('unavailable', 1))

    def test_invalid_png_is_unavailable(self):
        self.image.write_bytes(b'not an image')
        self.assertEqual(self.run_helper(), ('unavailable', 1))

    def observe(self, tool, trusted=False):
        env = dict(os.environ, RUWAC_GDM_OCR_TOOL=str(tool))
        argv = ['python3', str(HELPER), str(self.image)] + (['--trusted-sequence'] if trusted else [])
        result = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=7)
        return json.loads(result.stdout)['kind']

    def unbranded_ocr(self, entry_label='Password'):
        """No Fedora logo: the full frame shows GDM's "Not listed?" link; field crops read entry_label."""
        tool = self.root / 'unbranded-ocr'
        tool.write_text(f"""#!/usr/bin/python3
from pathlib import Path
import sys
print('left\\ttop\\twidth\\theight\\tconf\\ttext')
if Path(sys.argv[1]).name == 'layout.png':
    print('480\\t540\\t30\\t12\\t96\\tNot')
    print('514\\t540\\t44\\t12\\t96\\tlisted?')
else:
    print('30\\t30\\t150\\t30\\t99\\t{entry_label}')
""")
        tool.chmod(0o700)
        return tool

    def test_unbranded_user_list_only_leads_to_the_not_listed_click(self):
        self.assertEqual(self.observe(self.unbranded_ocr()), 'users')

    def small_link_ocr(self, recovered_confidence=96):
        tool = self.root / 'small-link-ocr'
        tool.write_text(f"""#!/usr/bin/python3
from pathlib import Path
import sys
print('left\\ttop\\twidth\\theight\\tconf\\ttext')
name = Path(sys.argv[1]).name
if name in ['layout.png', 'user-link.png']:
    confidence = 50 if name == 'layout.png' else {recovered_confidence}
    x,y = (480,540) if name == 'layout.png' else (24,18)
    print(f'{{x}}\\t{{y}}\\t30\\t12\\t{{confidence}}\\tNot')
    print(f'{{x+34}}\\t{{y}}\\t44\\t12\\t{{confidence}}\\tlisted?')
else:
    print('30\\t30\\t150\\t30\\t99\\tEmail')
""")
        tool.chmod(0o700)
        return tool

    def test_small_user_link_requires_high_confidence_crop_confirmation(self):
        self.assertEqual(self.observe(self.small_link_ocr()), 'users')
        self.assertEqual(self.observe(self.small_link_ocr(69)), 'unknown')

    def test_recovered_user_link_does_not_bypass_unknown_field_guard(self):
        self.focus_boxes(1)
        self.assertEqual(self.observe(self.small_link_ocr()), 'unknown')

    def test_unbranded_fields_need_a_sign_in_that_passed_the_login_screen(self):
        self.focus_boxes(1)
        tool = self.unbranded_ocr()
        # A user list with an input field visible is not the plain GDM list.
        self.assertEqual(self.observe(tool), 'unknown')
        self.assertEqual(self.observe(tool, trusted=True), 'password')
        self.assertEqual(self.observe(self.unbranded_ocr('Username'), trusted=True), 'username')

    def test_unbranded_user_list_with_any_focused_field_is_unknown(self):
        # Codex review: an unrecognized field (e.g. Email) must not establish
        # the GDM user list and so must never lead to trusted recognition.
        self.focus_boxes(1)
        self.assertEqual(self.observe(self.unbranded_ocr('Email')), 'unknown')

    def no_list_ocr(self, entry_label):
        """No Fedora logo and no user list: only field crops read entry_label."""
        tool = self.root / 'no-list-ocr'
        tool.write_text(f"""#!/usr/bin/python3
from pathlib import Path
import sys
print('left\\ttop\\twidth\\theight\\tconf\\ttext')
if Path(sys.argv[1]).name != 'layout.png':
    print('30\\t30\\t150\\t30\\t99\\t{entry_label}')
""")
        tool.chmod(0o700)
        return tool

    def test_unbranded_username_page_accepts_only_the_login_name_field(self):
        # GDM's "Not listed?" page (or a greeter without a user list): the
        # non-secret login name may be typed; a password needs the sequence.
        self.focus_boxes(1)
        self.assertEqual(self.observe(self.no_list_ocr('Username')), 'username')
        self.assertEqual(self.observe(self.no_list_ocr('Password')), 'unknown')
        self.assertEqual(self.observe(self.no_list_ocr('Email')), 'unknown')
        self.assertEqual(self.observe(self.no_list_ocr('Password'), trusted=True), 'password')

    def test_unbranded_username_page_with_two_fields_is_unknown(self):
        self.focus_boxes(2)
        self.assertEqual(self.observe(self.no_list_ocr('Username')), 'unknown')

    def test_large_framebuffer_is_cropped_at_full_resolution(self):
        # A HiDPI window gives GDM a large scale-1 framebuffer; the centred
        # form keeps its size and the rectangle maps back to the frame.
        image = Image.new('RGB', (3400, 2000), '#222222')
        ImageDraw.Draw(image).rectangle((1550, 1000, 1850, 1040), outline=(150, 180, 205))
        image.save(self.image)
        env = dict(os.environ, RUWAC_GDM_OCR_TOOL=str(self.no_list_ocr('Username')))
        result = subprocess.run(['python3', str(HELPER), str(self.image)],
                                env=env, capture_output=True, text=True, timeout=7)
        observation = json.loads(result.stdout)
        self.assertEqual(observation['kind'], 'username')
        x, y, r, b = observation['rect']
        self.assertTrue(1540 <= x <= 1560 and 995 <= y <= 1005 and 1840 <= r <= 1860 and 1035 <= b <= 1045,
                        observation)

    def test_two_focused_password_fields_are_ambiguous(self):
        self.focus_boxes(2)
        self.assertEqual(self.run_helper(self.fake_ocr()), ('ambiguous', 0))

    def test_low_confidence_placeholder_never_authorizes_input(self):
        self.focus_boxes(1)
        self.assertEqual(self.run_helper(self.fake_ocr(confidence=88)), ('unknown', 0))

    def test_low_confidence_brand_crop_never_authorizes_input(self):
        self.focus_boxes(1)
        self.assertEqual(self.run_helper(self.fake_ocr(branding_confidence=80)), ('unknown', 0))

    def test_malformed_ocr_never_authorizes_input(self):
        self.assertEqual(self.run_helper(self.fake_ocr(malformed=True)), ('unknown', 0))

    def test_timed_out_ocr_child_is_reaped(self):
        tool = self.root / 'slow-ocr'
        pid = self.root / 'child.pid'
        tool.write_text('#!/usr/bin/python3\nimport os,time\n'
                        f'open({str(pid)!r},"w").write(str(os.getpid()))\ntime.sleep(20)\n')
        tool.chmod(0o700)
        self.assertEqual(self.run_helper(tool), ('unavailable', 1))
        self.assertFalse(Path('/proc', pid.read_text()).exists())


if __name__ == '__main__':
    unittest.main()
