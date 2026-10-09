# Release v0.47.0

**Released:** 2026-10-09
**Previous version:** v0.46.0

Claude chats can now move to another Claude account mid-conversation, so a chat at one account's usage limit carries on under another without `/login`. Projects can also have their own default account.

## New Features

- Added an account picker to Claude chats, next to the model picker, on desktop and phone. Picking another account restarts the chat under that login with its full history; the next reply re-reads the conversation, so it starts with a cold cache (#250)
- When a chat hits a usage limit, the limit banner offers "Continue on _account_" for each of your other accounts (#250)
- Added a Claude account setting to Edit Project. New Claude sessions in that project start under it on desktop and phone, over the active account (#250)

## Notes

- A switch only happens between turns. If the other account can't start (for example it isn't logged in), the chat goes back to the account it was on, history intact (#250)
- Model picks don't carry across accounts, since the other account may not have that model (#250)
