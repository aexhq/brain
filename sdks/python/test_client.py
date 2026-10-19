import io
import json
import unittest
from urllib.error import HTTPError
from brain import Brain, BrainError, Model, ApiKey, CredentialReference, ManagedTool


class ClientTests(unittest.TestCase):
    def test_model_and_tool_authoring(self):
        client = Brain("http://brain.test", "token")
        bodies = []
        class Opener:
            def open(self, request, **kwargs):
                bodies.append(json.loads(request.data))
                return io.BytesIO(b'{"id":"session"}')
        client._opener = Opener()
        client.create(harness="pi", model=Model("test/model", ApiKey("customer-key")), tools=[
            ManagedTool("lookup"), ManagedTool("data", operations=["read"], alias="records", parameters={"bucket":"one"}, credential={"token":"tool-key"})])
        self.assertEqual(bodies, [{"harness":"pi", "config":{"model":"test/model", "tools":["lookup"], "bindings":[
            {"alias":"records", "provider":"data", "operations":["read"], "parameters":{"bucket":"one"}}]},
            "model_credential":{"api_key":"customer-key"}, "credentials":[{"binding":"records", "value":{"token":"tool-key"}}]}])
        self.assertEqual(CredentialReference("tenant-key"), {"reference":"tenant-key"})
        with self.assertRaisesRegex(ValueError, "require operations"):
            client.create(harness="pi", model=Model("test/model"), tools=[ManagedTool("lookup", credential="ignored")])

    def test_no_retry_and_error_identity(self):
        client = Brain("http://brain.test", "token")
        class Opener:
            calls = 0
            def open(self, request, **kwargs):
                self.calls += 1
                raise HTTPError(request.full_url, 429, "busy", {}, io.BytesIO(b'{"error":{"code":"overloaded","message":"busy"},"request_id":"request"}'))
        opener = Opener()
        client._opener = opener
        with self.assertRaises(BrainError) as raised:
            client.send("session", {"type": "user.message", "text": "hello"})
        self.assertEqual(opener.calls, 1)
        self.assertEqual(raised.exception.code, "overloaded")
        self.assertEqual(raised.exception.request_id, "request")

    def test_stream_comments_and_utf8(self):
        client = Brain("http://brain.test", "token")
        class Opener:
            def open(self, request, **kwargs):
                return io.BytesIO(': keepalive\n\ndata: {"id":"one","data":{"text":"你好"}}\n\n'.encode())
        client._opener = Opener()
        self.assertEqual(list(client.observe("session"))[0]["data"]["text"], "你好")


if __name__ == "__main__":
    unittest.main()

