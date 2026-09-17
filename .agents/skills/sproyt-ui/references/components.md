# Component contracts

Package exports: `@sproyt/ui/react`, `@sproyt/ui/vue`, `@sproyt/ui/styles.css`, `@sproyt/ui/tokens`.

## Shared catalog

| Component | Props and composition |
|---|---|
| Theme | `mode: light/dark/system`, `accent: citron/periwinkle`, `density: comfortable/compact`; children/default slot |
| Button | `variant: primary/secondary/quiet/danger`, `busy`, `disabled`; native button attributes/events. Default type is button; submit explicitly. |
| TextField | `label`, `hint`, `error`, `id`, native input attributes. React forwards a ref; Vue forwards attributes to the input. |
| AppShell | `view: list/detail`, `navigationLabel`; React `header`, `navigation`, children; Vue named header/navigation and default slots |
| ConversationList | `items: {id,name,group,unread?,muted?}[]`, `selectedId`, `emptyLabel`; selection callback/event |
| Message | `author`, `time`, `dateTime?`, `status?`; body children/default slot; React `actions`, Vue actions slot |
| Composer | controlled text, `label`, `placeholder`, `sendLabel`, `hint`, `disabled`, `busy`, `error`, `sendOnEnter`; React `leading` and `tools`, Vue leading/tools slots; `toolsLabel` labels the disclosure button |
| ThreadPane | `title`, `context`, `closeLabel`; React `parent`, `composer`, children, `onClose`; Vue parent/composer/default slots, `@close`. Escape closes; host restores focus. |
| EventCard | `title`, `when`, `where?`, controlled RSVP, `summary`, `label`, `options`, `disabled`; RSVP `yes/maybe/no` |
| Avatar | `name`; decorative initials |
| PersonList | `people: {id,name,detail?}[]`, `emptyLabel`; React actions(person), Vue actions slot with person |
| Dialog | `open`, `title`, `closeLabel`; React `onClose`, Vue `@close`; children/default slot |
| Status | `tone: info/error`; content children/default slot, live-region semantics |

`Composer` emits React `onSend(text)` / Vue `@send="handler"`. Ctrl/Cmd+Enter sends; plain Enter inserts a newline by default. `sendOnEnter` opts into Enter-send, Shift+Enter for newline. IME composition does not send. Disabled/busy/whitespace-only messages cannot send. The app controls clearing and retries.

## React

```tsx
import {useState} from 'react';
import {Theme, Composer, EventCard} from '@sproyt/ui/react';
import type {Rsvp} from '@sproyt/ui/tokens';
import '@sproyt/ui/styles.css';

export function Example() {
  const [text,setText] = useState('');
  const [reply,setReply] = useState<Rsvp>();
  return <Theme mode="dark">
    <EventCard title="Dinner" when="Saturday, 18:00"
      value={reply} onChange={setReply}
      options={[{value:'yes',label:'Going'},{value:'maybe',label:'Maybe'},{value:'no',label:'Cannot attend'}]} />
    <Composer label="Message" value={text} onChange={setText}
      onSend={message=>{ console.log(message); setText(''); }} />
  </Theme>;
}
```

## Vue

```vue
<script setup lang="ts">
import {ref} from 'vue';
import {Theme, Composer, EventCard} from '@sproyt/ui/vue';
import type {Rsvp} from '@sproyt/ui/tokens';
import '@sproyt/ui/styles.css';
const text=ref('');
const reply=ref<Rsvp>();
function send(message:string){console.log(message);text.value='';}
</script>
<template>
  <Theme mode="dark">
    <EventCard title="Middag" when="Laurdag, 18:00" v-model="reply" />
    <Composer label="Melding" v-model="text" @send="send" />
  </Theme>
</template>
```

The console-only examples intentionally have no external side effects. Replace them with the host application's authorized send behavior and handle asynchronous failures explicitly. The package is ESM with TypeScript declarations; Vue components are compiled render functions and work inside ordinary SFC templates. React exports carry a client boundary for applications that use React Server Components.

## Composing a responsive conversation

Wrap the channel and optional thread in `.sp-discussion` with `data-thread-open="true"` when a thread is open. The channel wrapper uses `.sp-channel-pane`. `ThreadPane` occupies the second column at content widths of 780px or more, and replaces the channel below that threshold. AppShell navigation switches to explicit list/detail at app container widths of 800px or less. These are container widths, so the same behavior works inside desktop widgets.

The host owns parent/reply IDs, separate drafts per conversation and thread, reply counts, and focus restoration. Keep replies out of the root timeline, provide a reply-count button on the parent, and return focus to that button when closing a thread. Keep the channel mounted to retain its reading position.

Composer starts at one line and grows to 3½ lines, then scrolls internally. It shrinks after clearing and reflows when its container width changes. Put optional writing commands in `tools`: focus or typing reveals them, and an explicit + button also toggles them. Leaving an empty composer hides them. Keep selected attachments, errors, and reply context in `leading`, where they remain visible independently of tool disclosure. Send stays visible. Tool contents and labels belong to the host.

## Emoji reaction popup
Both framework entry points export `openReactionPicker(anchor, options)` and `reactionEmoji`. The anchor is a message or reaction button HTMLElement inside Theme. Options require `onSelect(emoji)` and accept `selected`, `items` (emoji/accessible-label tuples), and localized `title`, `searchLabel`, `moreLabel`, `lessLabel`, `closeLabel`, `emptyLabel`. The returned function closes and disposes the popup. The starter catalog has 24 emoji; provide a larger localized catalog when needed.

`Message` accepts `onReactionRequest(anchor)` in React and Vue. This requests the picker on context-menu activation or a 500ms touch hold, cancelled by movement over 10px, pointer cancellation, or release. Keep a visible reaction button for keyboard and assistive technology. Native links and fields retain their context menus.

Example: `openReactionPicker(anchor, {selected: message.emoji, onSelect: emoji => toggleReaction(message.id, emoji)})`. The host owns toggling and persistence. The demo allows one local reaction per message, including thread replies. The dialog has no dimming backdrop, stays within viewport edges, supports Escape/outside dismissal, keyboard arrows in the emoji grid, native focus containment, and focus restoration.
