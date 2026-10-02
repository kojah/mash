import unittest
from unittest.mock import patch

from cqadup_capture import ETAG, Remote, SIZE


class RangeCaptureTests(unittest.TestCase):
    def test_random_access_and_eof_are_exact(self):
        calls = []
        def fetch(start,end):
            calls.append((start,end))
            return b'x'*(end-start+1)
        remote = Remote(fetch)
        self.assertEqual(remote.read(3),b'xxx')
        self.assertEqual(remote.tell(),3)
        remote.seek(-2,2)
        self.assertEqual(remote.read(9),b'xx')
        self.assertEqual(remote.read(),b'')
        self.assertEqual(calls,[(0,2),(SIZE-2,SIZE-1)])
        self.assertEqual(remote.transferred,5)
        self.assertEqual(remote.requests,2)

    def test_budget_invalid_seek_and_short_reads_fail(self):
        remote = Remote(lambda a,b:b'')
        for size in (-1,9*1024*1024):
            with self.assertRaises(ValueError):remote.read(size)
        with self.assertRaises(ValueError):remote.read(1)
        for offset,whence in ((-1,0),(0,3)):
            with self.assertRaises(ValueError):remote.seek(offset,whence)
        remote.transferred=384*1024*1024
        with self.assertRaises(ValueError):remote.read(1)

    def test_http_ignored_range_or_changed_archive_rejected_before_body_read(self):
        class Response:
            def __init__(self,status=206,etag=ETAG):
                self.status=status
                self.headers={'Content-Range':f'bytes 2-3/{SIZE}','ETag':etag}
                self.read_called=False
            def __enter__(self):return self
            def __exit__(self,*args):pass
            def read(self,size):self.read_called=True;return b'xx'
        for response in (Response(200),Response(etag='changed')):
            with patch('cqadup_capture.urllib.request.urlopen',return_value=response):
                with self.assertRaises(ValueError):Remote.request(2,3)
            self.assertFalse(response.read_called)
        response=Response()
        with patch('cqadup_capture.urllib.request.urlopen',return_value=response) as call:
            self.assertEqual(Remote.request(2,3),b'xx')
            self.assertEqual(call.call_args.args[0].get_header('If-match'),ETAG)


if __name__=='__main__':
    unittest.main()
