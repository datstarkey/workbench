# Release v0.39.1

**Released:** 2026-10-06
**Previous version:** v0.39.0

A round of fixes for Claude chats that run in a terminal. Messages, images and files sent while Claude is working now reach the running turn instead of waiting for it to end. The agents panel shows background agents as running, with their live output. The model picker lists the models your account actually has, and the prompt-cache timer shows the right lifetime.

## Bug Fixes

- Fixed messages sent while Claude was working waiting until the turn ended and then arriving all at once. They now join the running turn, as they do in the terminal. Images, PDFs and text files attached to them reach Claude too. A stopped turn no longer starts again for a queued message, and reloaded chats show the message as you typed it. (#170)
- Fixed the agents panel marking background agents as finished the moment they started. They now show as running until they really finish. (#170)
- Fixed the agents panel saying an agent "wrote no output". It now shows the agent's live output, and parallel agents each show their own progress. (#171)
- Fixed the chat's model picker showing a guessed list. It now lists the models your account can use in that project, with each model's real effort levels. It also shows the model you have picked, and the model and effort a turn actually ran with. The list is checked once an hour without calling a model, and an app restart checks it again. (#170)
- Fixed the prompt-cache timer showing 5 minutes when the cache really lasted an hour. (#170)
- Fixed "Rewind to here" showing an error in terminal chats. It now explains that files can't be restored there yet and still offers rewinding the conversation. (#171)

## Notes

- These fixes are in the Workbench plugin that terminal chats run. Claude sessions already running keep the old plugin until they restart.
