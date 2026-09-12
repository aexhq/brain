# Native media fixtures

`diagram.png` contains two red circles. `diagram.pdf` contains an embedded image of three blue
squares and no answer text. The live probe checks these counts in user input, Tool results,
continuation and Responses compaction using HTTPS URLs at the tested commit.

`diagram-vector.pdf` preserves the same three blue squares as vector drawings. Direct OpenAI and
Vercel Responses did not recognize its visual content in the September 2026 controls, while the
image-based PDF passed. Keep it for reproducing that provider compatibility issue; the release
probe uses `diagram.pdf`.
