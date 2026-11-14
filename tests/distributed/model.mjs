import http from 'node:http';
import { responsesReply } from '../journeys/model-responses.mjs';

http.createServer(async (req, res) => {
  if (req.url === '/health') { res.end('ok'); return; }
  if (req.url === '/reference') { res.writeHead(200,{'Content-Type':'text/plain'}).end('authorized external reference'); return; }
  if (req.url === '/echo') { let body='';for await (const chunk of req)body+=chunk;const input=JSON.parse(body);res.writeHead(200,{'Content-Type':'application/json'}).end(JSON.stringify({content:[{type:'text',text:input.arguments.value}]}));return; }
  if (req.url !== '/v1/responses') { res.writeHead(404).end(); return; }
  try {
    let data = '';
    for await (const chunk of req) { data += chunk; if (data.length > 4 * 1024 * 1024) throw new Error('request too large'); }
    const input = JSON.parse(data);
    const last = input.input.at(-1);
    const text = typeof last.content === 'string' ? last.content : (last.content ?? []).filter(p => ['input_text', 'output_text', 'text'].includes(p.type)).map(p => p.text).join('');
    if (text === 'BLOCK until cancelled') {
      const timer = setTimeout(() => res.end(), 60000);
      res.once('close', () => clearTimeout(timer));
      return;
    }
    const call = last.role === 'user' && /^CALL (\w+) (.+)$/s.exec(text);
    const answer = call ? { name: call[1], args: JSON.parse(call[2]) } : last.type === 'function_call_output' ? 'tool handled' : `answer: ${text}`;
    res.modelStreaming = input.stream;
    responsesReply(res, answer, { input_tokens: 10, output_tokens: 5, total_tokens: 15, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } });
  } catch {
    res.writeHead(500).end('fixture failed');
  }
}).listen(8089, '0.0.0.0');
