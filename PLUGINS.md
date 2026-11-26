# Ember Plugins

Port targets for Ember's native-Rust plugin system. The list is every plugin
shipped in **Vencord** (167), since that's our source to port *from*. We write
each one ourselves in Rust — we don't reuse Sunbird's or Vencord's code — but
Sunbird's native ports and Vencord's source are both useful references for *how*
a plugin behaves.

**silent-typing is ported** (the first slice, via Ember's request hook). Every
other row is for you to triage: decide if it's worth porting, mark its status, and
for any that genuinely can't work on the native client, write **Not possible**.

## Columns

- **Status** — blank = not yet triaged · `✅ Ported` · `🔜 Worth porting` ·
  `❌ Not possible` (and why, if useful). Only silent-typing is marked so far.
- **Description** — what the plugin does (from Vencord's own description text).
- **Hook type** — a *rough automatic guess* at which Ember capability it needs, to
  help plan effort. **Verify before trusting it.** Meanings:
  - `request` — outgoing HTTP hook (Ember has this). Easiest; silent-typing is here.
  - `gateway` — incoming server events. Needs the gateway hook (not built yet).
  - `command` — a slash command.
  - `context-menu` — adds right-click menu items.
  - `settings-only` — mostly a config toggle, little hooking.
  - `UI patch` — modifies Discord's rendered UI. **Hardest on Phoenix**: Vencord
    patches Discord's React/web UI, but Phoenix is native GPUI with no React, so
    most of these need native UI work or may be **Not possible**. 131 of 167 fall
    here — expect this to be the bulk of the triage.
  - `?` — couldn't guess; inspect the plugin.

## Reality check on scope

Vencord plugins patch an Electron/React client. Phoenix is a native Rust/GPUI
client. Plugins that only change behavior (request/gateway hooks) port cleanly;
plugins that re-skin or restructure the UI do not map 1:1 and are the ones most
likely to be **Not possible** or to need bespoke native work. Triage with that in
mind.

## BetterDiscord

The cloned BetterDiscord repo is only the client/loader — it bundles **no
plugins**. BD's plugins are thousands of community add-ons hosted on
betterdiscord.app, not in-repo, so they're not listed here. They're a later pool
to draw from if a specific BD plugin is wanted.

## Sunbird-only plugins (not in Vencord)

Sunbird shipped a few native plugins with no Vencord equivalent, worth adding
later: **taptap** (mobile-style double-tap to like) and **global-badges** (shows
badges from other client mods). `fix-embeds` and `hide-composer-buttons` partly
overlap Vencord but aren't direct matches.

## Plugins (from Vencord — 167)

| Plugin | Status | Description | Hook type (guess) |
| --- | --- | --- | --- |
| AccountPanelServerProfile | | Prioritize Server Profile when left clicking your account panel | UI patch |
| AddAttachments | | Allows you to add new attachments while editing messages | ? |
| AlwaysAnimate | | Animates anything that can be animated | UI patch |
| AlwaysExpandRoles | | Always expands the role list in profile popouts | UI patch |
| AlwaysTrust | | Remove the untrusted domain popup when opening links | UI patch |
| AnonymiseFileNames | | Whether to anonymise file names by default | UI patch |
| WebRichPresence (arRPC) | | Client plugin for arRPC to enable RPC on Discord Web (experimental) | gateway |
| AutoDNDWhilePlaying | | Status to set while playing a game | gateway |
| BetterFolders | | Display servers from folder on dedicated sidebar | UI patch |
| BetterGifAltText | | Change GIF alt text from simply being 'GIF' to containing the gif tags / filename | UI patch |
| BetterGifPicker | | Makes the gif picker open the favourite category by default | UI patch |
| BetterRoleContext | | File format to use when viewing role icons | UI patch |
| BetterRoleDot | | Show both role dot and coloured names | UI patch |
| BetterSearch | | Allows you to use channel message search options that aren't normally available | UI patch |
| BetterSessions | | Check for new sessions in the background, and display notifications when they are detected | UI patch |
| BetterSettings | | Disable the crossfade animation | UI patch |
| BetterUploadButton | | Upload with a single click, open menu with right click | UI patch |
| BetterYouTubeEmbeds | | Restore videos blocked from displaying in Discord, for example by UMG | settings-only |
| BiggerStreamPreview | | This plugin allows you to enlarge stream previews | context-menu |
| BlurNSFW | | Blur Amount (in pixels) | UI patch |
| CallTimer | | The timer format. This can be any valid moment.js format | UI patch |
| CharacterCounter | | Enable yellow/red colouring as you get closer to the character limit | UI patch |
| ClearURLs | ✅ Ported | Automatically removes tracking elements from URLs you send | request |
| ClientTheme | | Recreation of the old client theme experiment. Add a color to your Discord client theme | settings-only |
| ColorSighted | | Removes the colorblind-friendly icons from statuses, just like 2015-2017 Discord | UI patch |
| ConsoleJanitor | | Disables Discords loggers | UI patch |
| ConsoleShortcuts | | Adds shorter Aliases for many things on the window. Run `shortcutList` for a list. | UI patch |
| CopyEmojiMarkdown | | Copy the raw unicode character instead of :name: for default emojis (👽) | context-menu |
| CopyFileContents | | Adds a button to text file attachments to copy their contents | UI patch |
| CopyUserURLs | | Adds a 'Copy User URL' option to the user context menu. | context-menu |
| CrashHandler | | Whether to attempt to prevent Discord crashes. | UI patch |
| ephemeral | | Whether the response should only be visible to you | request |
| CustomIdle | | Minutes before Discord goes idle (0 to disable auto-idle) | UI patch |
| CustomRPC | | Whether to show the presence | UI patch |
| Dearrow | | Hides the Dearrow button from YouTube embeds | UI patch |
| Decor | | Create and use your own custom avatar decorations, or pick your favorite from the presets. | UI patch |
| DevCompanion | | Whether to notify when Dev Companion has automatically connected. | UI patch |
| DirectFileDownload | | Directly download files from Discord instead of opening them in the browser | UI patch |
| DisableCallIdle | | Disables automatically getting kicked from a DM voice call after 3 minutes and being moved t... | UI patch |
| DontRoundMyTimestamps | | Always rounds relative timestamps down, so 7.6y becomes 7y instead of 8y | ? |
| Experiments | | Enable Access to Experiments & other dev-only features in Discord! | UI patch |
| ExpressionManager | | Clone Emotes & Stickers. Adds context menu options to quickly clone, edit, or delete them. A... | UI patch |
| F8Break | | Pause the client when you press F8 with DevTools (+ breakpoints) open. | ? |
| FakeNitro | | Allows sending fake emojis (also bypasses missing permission to use custom emojis) | UI patch |
| FakeProfileThemes | | Default color source if both are present | UI patch |
| FavoriteEmojiFirst | | Puts your favorite emoji first in the emoji autocomplete. | UI patch |
| FixCodeblockGap | | Removes the gap between codeblocks and text below it | UI patch |
| FixImagesQuality | | Also load the original image in Chat. WARNING: Read the caveats above | UI patch |
| FixSpotifyEmbeds | | The volume % to set for spotify embeds. Anything above 10% is veeeery loud | settings-only |
| ForceOwnerCrown | | Force the owner crown next to usernames even if the server is large. | UI patch |
| FriendInvites | | Create and manage friend invite links via slash commands (/create friend invite, /view frien... | command |
| FullSearchContext | | Makes the message context menu in message search results have all options you'd expect | UI patch |
| GameActivityToggle | | Use the old icon style before Discord icon redesign | UI patch |
| GifPaste | | Makes picking a gif in the gif picker insert a link into the chatbox instead of instantly se... | UI patch |
| GifProviderSwitcher | | The provider to use. You can also change this directly in the GIF picker | UI patch |
| GreetStickerPicker | | Choose the greet mode | UI patch |
| HideMedia | | Hide attachments and embeds for individual messages via hover button | UI patch |
| IgnoreActivities | | Change the mode of the filter list | UI patch |
| iLoveSpam | | Do not hide messages from 'likely spammers' | UI patch |
| ImageFilename | | Show the full URL of the image instead of just the file name. Always enabled for GIFs becaus... | UI patch |
| ImageLink | | Never hide image links in messages, even if it's the only content | UI patch |
| ImageZoom | | Whether to save zoom and lens size values | UI patch |
| ImplicitRelationships | | Whether to sort implicit relationships by their affinity to you. | UI patch |
| IrcColors | | Lightness, in %. Change if the colors are too light or too dark | UI patch |
| JumpToTop | | Adds a button to jump to the first message in a channel | UI patch |
| KeepCurrentChannel | | Attempt to navigate to the channel you were in before switching accounts or loading Discord. | UI patch |
| LoadingQuotes | | Should this plugin also apply during events with special event themed quotes? (e.g. Halloween) | UI patch |
| MemberCount | | Show member count on the server tooltip | UI patch |
| MentionAvatars | | Whether the the @ symbol should be displayed on user mentions | UI patch |
| MessageClickActions | | Enable delete on click while holding backspace | gateway |
| MessageLatency | | Displays an indicator for messages that took ≥n seconds to send | UI patch |
| MessageLinkEmbeds | | Background color for messages in rich embeds | UI patch |
| MessageLogger | | The style of deleted messages | UI patch |
| MoreQuickReactions | | Number of reactions (0-42) | UI patch |
| MusicRichPresence | | The scrobbler backend to use. | gateway |
| MutedServersActivityBadge | | Keeps the servers activity badge visible even when server is muted. | UI patch |
| MutualGroupDMs | | Shows mutual group dms in profiles | UI patch |
| NewGuildSettings | | Mute Guild automatically | UI patch |
| NoBlockedMessages | | Completely ignores incoming messages from blocked and ignored (if enabled) users | UI patch |
| DisableDeepLinks | | Disables Discord's stupid deep linking feature which tries to force you to use their Desktop... | UI patch |
| NoDevtoolsWarning | | Disables the 'HOLD UP' banner in the console. As a side effect, also prevents Discord from h... | UI patch |
| NoF1 | | Disables F1 help bind. | UI patch |
| NoMaskedUrlPaste | | Pasting a link while having text selected will not paste as masked URL | UI patch |
| NoMiddleClickPaste | | Disable Linux middle-click paste - Linux only | ? |
| NoMosaic | | Play videos without carousel modal | UI patch |
| NoOnboardingDelay | | Skips the slow and annoying onboarding delay | UI patch |
| NoPendingCount | | Hide incoming friend requests count | UI patch |
| NoProfileThemes | | Completely removes Nitro profile themes from everyone but yourself | UI patch |
| NoReplyMention | | List of user ids to allow or exempt pings for (separated by commas or spaces) | UI patch |
| NoServerEmojis | | The types of emojis to show in the autocomplete menu. | UI patch |
| NoSystemBadge | | Disables the taskbar and system tray unread count badge. | UI patch |
| NotificationVolume | | Notification volume | UI patch |
| NoTypingAnimation | | Disables the CPU-intensive typing dots animation | UI patch |
| NoUnblockToJump | | Allows you to jump to messages of blocked or ignored users and likely spammers without unblo... | UI patch |
| oneko | | cat follow mouse (real) | ? |
| OnePingPerDM | | Select the type of DM for the plugin to affect | UI patch |
| OpenInApp | | Open Spotify links in the Spotify app | UI patch |
| OverrideForumDefaults | | Which layout to use as default | UI patch |
| PauseInvitesForever | | Brings back the option to pause invites indefinitely that stupit Discord removed. | UI patch |
| PermissionFreeWill | | Disables the client-side restrictions for channel permission management. | UI patch |
| PermissionsViewer | | The sort method used for defining which role grants an user a certain permission | UI patch |
| petpet | | Adds a /petpet slash command to create headpet gifs from any image | command |
| PictureInPicture | | Whether to make the PiP video loop or not | UI patch |
| PinDMs | | Which order should pinned DMs be displayed in? | UI patch |
| PlainFolderIcon | | Dont show the small guild icons in folders | UI patch |
| PlatformIndicators | | In the member list | UI patch |
| PreviewMessage | | Lets you preview your message before sending it. | gateway |
| QuickMention | | Adds a quick mention button to the message actions bar | ? |
| QuickReply | | Ping reply by default | gateway |
| ReactErrorDecoder | |  | UI patch |
| ReadAllNotificationsButton | | Read all server notifications with a single button click! | gateway |
| RelationshipNotifier | | Notifies you when a friend, group chat, or server removes you. | UI patch |
| ReplaceGoogleSearch | | Name of the custom search engine | context-menu |
| ReplyTimestamp | | Shows a timestamp on replied-message previews | UI patch |
| RevealAllSpoilers | | Reveal all spoilers in a message by Ctrl-clicking a spoiler, or in the chat with Ctrl+Shift-... | UI patch |
| ReverseImageSearch | | Adds ImageSearch to image context menus | UI patch |
| ReviewDB | | Review other users (Adds a new settings to profiles) | UI patch |
| RoleColorEverywhere | | Show role colors in chat mentions (including in the message box) | UI patch |
| SecretRingToneEnabler | | Only play the Snow Halation Theme | UI patch |
| SendTimestamps | ✅ Ported | Replace timestamps in message contents | request |
| ServerInfo | | Allows you to view info about a server | context-menu |
| ServerListIndicators | | Mode | settings-only |
| ShikiCodeblocks | | Brings vscode-style codeblocks into Discord, powered by Shiki | UI patch |
| ShowAllMessageButtons | | Always show all message buttons no matter if you are holding the shift key or not. | UI patch |
| ShowConnections | | Icon size (px) | UI patch |
| ShowHiddenChannels | | The mode used to display hidden channels. | UI patch |
| ShowHiddenThings | | Displays various hidden & moderator-only things regardless of permissions. | UI patch |
| ShowMeYourName | | How to display usernames and nicks | UI patch |
| ShowTimeoutDuration | | How to display the timeout duration | UI patch |
| SilentMessageToggle | | Whether to persist the state of the silent message toggle when changing channels | request |
| SilentTyping | ✅ Ported | Stop sending typing indicators to others | request |
| SortFriendRequests | | Show dates on friend requests | UI patch |
| SpotifyControls | | Show controls on hover | UI patch |
| SpotifyCrack | | Disable Spotify auto-pause | UI patch |
| SpotifyShareCommands | | Share your current Spotify track, album or artist via slash command (/track, /album, /artist) | request |
| StartupTimings | | Adds Startup Timings to the Settings menu | ? |
| StickerPaste | | Makes picking a sticker in the sticker picker insert it into the chatbox instead of instantl... | UI patch |
| StreamerModeOnStream | | Automatically enables streamer mode when you start streaming in Discord | gateway |
| SuperReactionTweaks | | Reaction picker will default to Super Reactions | UI patch |
| TextReplace | ✅ Ported | Rules for replacing text using string matching. | UI patch |
| ThemeAttributes | | Adds data attributes to various elements for theming purposes | UI patch |
| Translate | | Translate messages with Google Translate, DeepL or Kagi. | request |
| TypingIndicator | | Whether to show the typing indicator for the currently selected channel | UI patch |
| TypingTweaks | | Show avatars in the typing indicator | UI patch |
| Unindent | ✅ Ported | Trims leading indentation from codeblocks | UI patch |
| UnlockedAvatarZoom | | Zoom multiplier | UI patch |
| UnsuppressEmbeds | | Allows you to unsuppress embeds in messages | context-menu |
| UserMessagesPronouns | | Adds pronouns to chat user messages | UI patch |
| UserVoiceShow | | Show a user's Voice Channel indicator in their profile next to the name | UI patch |
| USRBG | | Banner to use if both Nitro and USRBG banners are present | UI patch |
| ValidReply | |  | UI patch |
| ValidUser | | Active Developer | UI patch |
| VoiceChatDoubleClick | | Join voice chats via double click instead of single click | UI patch |
| VcNarrator | | Announces when users join, leave, or move voice channels via narrator | gateway |
| VencordToolbox | | Show the plugins menu in the toolbox | UI patch |
| ViewIcons | | Choose the image format to use for non-animated images. Animated images will always use .webp | UI patch |
| Guild | | Change the button to view the raw content/data of any message. | context-menu |
| VoiceDownload | | Adds a download to voice messages. (Opens a new browser tab) | UI patch |
| VoiceMessages | | Allows you to send voice messages like on mobile. To do so, right click the upload button an... | UI patch |
| VolumeBooster | | Volume Multiplier | UI patch |
| WebContextMenus | | Add back the Discord context menus for images, links and the chat input bar | UI patch |
| WebKeybinds | | Show the back/forward navigation buttons in the title bar. | UI patch |
| Discord | | Imagine a place... | UI patch |
| WebScreenShare | | Content Hint | UI patch |
| WebScreenShareFixes | | MAY CAUSE ISSUES! Prefer better codecs like AV1 and VP9 when streaming. Should improve quali... | UI patch |
| WhoReacted | | While this is enabled, clicking a reacting user's avatar will open their profile instead of ... | UI patch |
| XSOverlay | | Websocket port | gateway |
