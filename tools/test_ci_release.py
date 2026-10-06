import unittest
from ci_release import release_kind,version_code,validate_badging


class ReleasePolicyTest(unittest.TestCase):
    def test_only_main_and_semantic_version_tags_can_publish(self):
        self.assertEqual(release_kind("refs/heads/main"),("preview","0.1.0"))
        self.assertEqual(release_kind("refs/tags/v1.2.3"),("release","1.2.3"))
        self.assertEqual(release_kind("refs/tags/v1.2.3-rc.1"),("release","1.2.3-rc.1"))
        for ref in ["refs/pull/1/merge","refs/heads/codex/work","refs/tags/v1","refs/tags/v01.2.3","refs/tags/v1.2.3;echo x","refs/tags/v1.2.3\n"]:
            with self.subTest(ref=ref),self.assertRaises(ValueError):release_kind(ref)

    def test_android_version_code_must_fit_the_platform(self):
        self.assertEqual(version_code("123"),123)
        for raw in ["0","-1","2100000001","1.5","１２","1\n"]:
            with self.subTest(raw=raw),self.assertRaises(ValueError):version_code(raw)

    def test_releases_reject_wrong_package_version_and_debug_apks(self):
        text="package: name='com.motionstudio.editor' versionCode='12' versionName='1.2.3'\napplication: label='Motion Studio'\n"
        validate_badging(text,12,"1.2.3")
        for invalid in [text.replace("com.motionstudio.editor","com.motionstudio.editor.effectsacceptance"),text.replace("versionCode='12'","versionCode='11'"),text.replace("1.2.3","1.2.2"),text+"application-debuggable\n"]:
            with self.subTest(text=invalid),self.assertRaises(ValueError):validate_badging(invalid,12,"1.2.3")


if __name__=="__main__":unittest.main()
