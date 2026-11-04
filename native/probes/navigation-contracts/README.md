# Native navigation contracts

This executable links the actual credential-free navigation owner from the
Windows app. It exercises canonical directory/search separation, circle IDs,
server-activity DM ranking, interaction stability, back/forward anchors,
account/family/token-rotation fences, access loss and optional account-owned
preference storage. It does not simulate successful transport or draft storage.

Run from this directory with its pinned SDK and a separate build area:

```powershell
dotnet run --project NavigationContracts.csproj --artifacts-path S:/Build/native-parity-navigation/managed -- S:/Build/native-parity-navigation/tests
```

The WinUI project includes the new application files by its normal source glob.
The pure controller needs no project/runtime change. The integrator owns the
actual `LiveApplication`/`LiveSnapshot`/`LiveChatComponent` hookup and UI tests.

Application seam:

1. `BindScope(userId, sessionStamp)` from the verified active/offline owner.
   Logout passes null. History survives token rotation in the same family;
   account or login-family changes clear it. Concrete callbacks still require
   the full stamp and monotonic visit generation.
2. `UpdateChannels(expectedUser, expectedStamp, canonicalChannels)` only from a
   complete, same-owner authoritative directory. Search results must never be
   sent here. Unchanged snapshots do not emit another `Changed` event.
3. `Select(target, outgoingAnchor)`, `Back(outgoingAnchor)` and
   `Forward(outgoingAnchor)` return a `NativeNavigationVisit`. The application
   performs any necessary channel/history operation and only completes/restores
   it if `IsCurrent(visit)` remains true. A current failure goes to `Fail`.
   `ChannelChanged` describes the logical transition. The current application
   refreshes each pending visit's transport view generation, including same-
   channel targets, so an earlier visible-read callback cannot enter it.
   A precise target still needs authorized history loading by the application;
   this owner does not manufacture data.
4. `SaveAnchor(currentVisit, anchor)` accepts only current-generation reports.
   An anchor contains stable message ID, channel sequence, row Y-offset relative
   to the viewport, `AtEnd` and absolute-offset fallback. Root owns the actual
   WinUI viewport controller and combines this with transport `ViewGeneration`.
5. Render `NativeNavigationView.Render(snapshot, palette, actions)` and supply
   application-owned callbacks. Its search never alters canonical data. The
   optional interaction callback freezes existing DM targets while pointer or
   keyboard focus is inside navigation. The application can also retain this
   freeze while a composer is being edited.
6. `RestartCurrent()` creates a fresh visit after reconnect/token rotation
   without duplicating history. `AdmitCurrent(visit, registration)` atomically
   registers the application operation only while the visit is current. Keep
   registration synchronous and short; cancel the old operation outside it.

Access loss prunes revoked channels from history and cached anchors. If the
current channel is removed, `Current` becomes null; Back/Forward may still open
other authorized entries. The application chooses an authorized fallback or
shows the directory. Drafts remain owned by `ChatWorkspace` throughout.

`NativeNavigationPreferenceStore` stores only directory scope in an account-keyed
versioned file. It never stores messages, channel names, queries or credentials.
Storage failure is nonfatal and visible as `PreferenceError`.
