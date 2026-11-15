# Local models with the Assistant

You can use a model running on your computer to work on the open design in
SolidCraft's desktop Assistant. See [Agent chat](agent-chat.md) for the panel's
controls.

## What you need

- Ollama, LM Studio, or another OpenAI-compatible server running on your computer.
- A model that supports **tool calling**: this lets it ask SolidCraft to read or
  change the design. On Ollama's model page, look for the **tools** tag, or run
  `ollama show <model>` and look for `tools` under capabilities. Tool support alone
  does not guarantee good results; see the table below.
- Enough graphics memory (VRAM) for the model and its context (the conversation
  and tool instructions). See the table below for sizes.

## Set it up

1. Start your server and load or download the model you want to use.
2. In SolidCraft, open **Preferences > AI > Add agent…** and choose the
   **OpenAI-compatible API**. Choose Ollama, LM Studio, or a custom URL.
3. Enter the server's base URL, such as `http://localhost:11434/v1` for Ollama,
   and the model name. Use **Test**, then add the endpoint.
4. Open the **Assistant** and select your endpoint. Choose the model in the model
   list if the server provides one.
5. Choose permissions in Preferences or the chat's permission menu:
   **Ask** asks before changes and file actions; **Edits** allows model edits but
   still asks before file actions and replacing the design; **All** allows actions
   without approval, including saving, overwriting, and replacing the design.

## Which models work?

These results were measured with the standard test (v1) in
[#4851](https://github.com/tangency-labs/solidcraft/issues/4851). The table in that
issue is the up-to-date one.

| Model | Size / quant | Passed (v1) |
|---|---|---|
| Muse Glimmer 30B | Unsloth UD-Q3_K_XL, 12.4 GB | **11/11** |
| Qwen3.8 27B | Unsloth UD-Q4_K_XL | **11/11** |
| gemma4 (latest, 7.5B) | Q4_K_M, 6.1 GB | **11/11** |
| gemma4:12b | Q4_K_M | 10/11 |
| gpt-oss:20b | MXFP4, 12.8 GB | 10/11 |
| qwen3:14b | Q4_K_M, 8.6 GB | 8/11 |
| qwen3:8b | Q4_K_M | 7/11 |
| qwen2.5:7b | Q4_K_M, 4.4 GB | 4/11 |
| qwen2.5-coder:7b | Q4_K_M, 4.4 GB | 0/11 (writes every call as text) |
| gemma3:4b | Q4_K_M | 0/11 (no tool support: HTTP 400) |
| deepseek-r1:14b | Q4_K_M | 0/11 (never calls a tool) |

For about 8 GB of graphics memory, start with `gemma4` (the default tag, 6.1 GB). With
16 GB, Muse Glimmer 30B or gpt-oss:20b also work well.

## Troubleshooting

- **The model replies with JSON or code, but nothing happens.** It is not calling
  tools well. Try another model from the results above.
- **“HTTP 400 … does not support tools.”** The model lacks tool support. Choose
  one with the **tools** capability.
- **Empty replies.** The context may be too small for the tool instructions.
  Increase the server's context size (`num_ctx` in Ollama), then try a new chat.
- **Slow answers.** The model may be too big for your VRAM and spilling work to
  the CPU. Try a smaller model or a smaller quantization.

## Measure a model yourself

Follow [the standard test in #4851](https://github.com/tangency-labs/solidcraft/issues/4851)
to compare your model with these results. It gives the exact prompts, starting
designs, pass rules, and a template for sharing results.
