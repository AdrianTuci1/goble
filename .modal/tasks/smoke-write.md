# Write smoke test

You are running inside a detached Modal container as a smoke test for the Goble
agent harness. This test checks that the agent can write a file and commit it
through the harness, with the default model.

1. Create `SMOKE-WRITE.md` in the repository root with exactly this content:

   ```markdown
   # Write smoke test

   Written by the Goble agent through the Modal harness.
   ```

2. Read the file back and confirm the content.
3. Commit only that file on the current branch, with the message
   `test(smoke): write through the harness`.
4. In `/work/SUMMARY.md`, include a **Model** section with the model id the
   session uses, if it is visible.

Do not run cargo. Do not touch any other file.
