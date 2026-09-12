# Native media fixtures

`diagram.png` contains two red circles. `report.pdf` contains the text `Report code: HARBOR-7391`.
The live probe reads the image count and PDF code through user input, Tool results, continuation
and Responses compaction using HTTPS URLs at the tested commit. The prompt and Tool-result text
do not supply the report code.

OpenAI and Anthropic models use Vercel's Responses endpoint with `VERCEL_AI_GATEWAY_API_KEY`;
the direct OpenAI reference check uses `OPENAI_API_KEY`. Anthropic Messages payloads are verified
by the regular adapter tests, independently of the live gateway check.
The probe verifies native attachment delivery and continuation, without testing PDF drawing recognition.
