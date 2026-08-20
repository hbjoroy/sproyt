// @ts-nocheck
// Transitional extraction: this is deliberately a behavior-preserving move of the former inline
// module. It is kept untyped only until the next migration stage splits DOM handles, transport,
// and state into typed modules. Do not add new application behaviour here.
      import { createApplicationStore, createServerEventMailbox } from "./client-store";

      function syncAppViewportHeight() {
        const viewport = window.visualViewport;
        const height = viewport?.height || window.innerHeight;
        const offsetTop = viewport?.offsetTop || 0;
        document.documentElement.style.setProperty("--app-height", `${Math.round(height)}px`);
        document.documentElement.style.setProperty("--app-offset-top", `${Math.round(offsetTop)}px`);
      }
      syncAppViewportHeight();
      window.addEventListener("resize", syncAppViewportHeight, { passive: true });
      window.visualViewport?.addEventListener("resize", syncAppViewportHeight, { passive: true });
      window.visualViewport?.addEventListener("scroll", syncAppViewportHeight, { passive: true });

      const serviceWorkerReady = "serviceWorker" in navigator
        ? navigator.serviceWorker.register("/service-worker.js", { scope: "/" }).then(() => navigator.serviceWorker.ready)
        : Promise.resolve(null);
      const connectForm = document.querySelector("#connect-form");
      const sendForm = document.querySelector("#send-form");
      const composerTools = document.querySelector("#composer-tools");
      const channelInput = document.querySelector("#channel");
      const bodyInput = document.querySelector("#body");
      const mentionSuggestions = document.querySelector("#mention-suggestions");
      const sendButton = document.querySelector("#send");
      const attachMediaButton = document.querySelector("#attach-media");
      const messageEmojiPicker = document.querySelector(".emoji-picker");
      const statusText = document.querySelector("#status-text");
      const statusEmoji = document.querySelector("#status-emoji");
      const currentStatus = document.querySelector("#current-status");
      const notificationSummary = document.querySelector("#notification-summary");
      const notificationMode = document.querySelector("#notification-mode");
      const notificationDirect = document.querySelector("#notification-direct");
      const notificationMentions = document.querySelector("#notification-mentions");
      const notificationNotice = document.querySelector("#notification-notice");
      const enableNotifications = document.querySelector("#enable-notifications");
      const mediaInput = document.querySelector("#media-input");
      const mediaPreviews = document.querySelector("#media-previews");
      const uploadStatus = document.querySelector("#upload-status");
      const mediaLightbox = document.querySelector("#media-lightbox");
      const mediaLightboxImage = document.querySelector("#media-lightbox-image");
      const mediaLightboxCaption = document.querySelector("#media-lightbox-caption");
      const threadPanel = document.querySelector("#thread-panel");
      const threadMessages = document.querySelector("#thread-messages");
      const threadForm = document.querySelector("#thread-form");
      const threadBody = document.querySelector("#thread-body");
      const threadEmojiPicker = document.querySelector("#thread-emoji-picker");
      const threadComposerTools = document.querySelector("#thread-composer-tools");
      const threadAttachMediaButton = document.querySelector("#thread-attach-media");
      const threadMediaInput = document.querySelector("#thread-media-input");
      const threadMediaPreviews = document.querySelector("#thread-media-previews");
      const threadUploadStatus = document.querySelector("#thread-upload-status");
      const threadSendButton = document.querySelector("#thread-send");
      const circleChannelDialog = document.querySelector("#circle-channel-dialog");
      const circleChannelTitle = document.querySelector("#circle-channel-title");
      const circleJoinableList = document.querySelector("#circle-joinable-list");
      const circleChannelCreate = document.querySelector("#circle-channel-create");
      const managedChannelName = document.querySelector("#managed-channel-name");
      const managedChannelKind = document.querySelector("#managed-channel-kind");
      const leaveCircleButton = document.querySelector("#leave-circle");
      const circleMembershipNotice = document.querySelector("#circle-membership-notice");
      const viewModeToggle = document.querySelector("#view-mode-toggle");
      const statusEl = document.querySelector("#status");
      const connectionStatusToggle = document.querySelector("#connection-status-toggle");
      const connectionStatusDot = document.querySelector("#connection-status-dot");
      const messagesEl = document.querySelector("#messages");
      const bottomChannelPanel = document.querySelector("#bottom-channel-panel");
      const bottomCirclePanel = document.querySelector("#bottom-circle-panel");
      const bottomChannelToggle = document.querySelector("#bottom-channel-toggle");
      const bottomCircleToggle = document.querySelector("#bottom-circle-toggle");
      const bottomNavigation = document.querySelector(".bottom-navigation");
      const bottomChannelList = document.querySelector("#bottom-channel-list");
      const bottomCircleList = document.querySelector("#bottom-circle-list");
      const bottomCircleContent = document.querySelector("#bottom-circle-content");
      const circleToolDirect = document.querySelector("#circle-tool-direct");
      const circleToolShared = document.querySelector("#circle-tool-shared");
      const circleToolSettings = document.querySelector("#circle-tool-settings");
      const circleAdminDialog = document.querySelector("#circle-admin-dialog");
      const circleAdminClose = document.querySelector("#circle-admin-close");
      const directMessageDialog = document.querySelector("#direct-message-dialog");
      const directUser = document.querySelector("#direct-user");
      const directMessageStatus = document.querySelector("#direct-message-status");
      const openDirect = document.querySelector("#open-direct");
      const conversationTitle = document.querySelector("#conversation-title");
      const conversationCircle = document.querySelector("#conversation-circle");
      const conversationContext = document.querySelector("#conversation-context");
      const conversationPeerStatus = document.querySelector("#conversation-peer-status");
      const channelPeopleButton = document.querySelector("#channel-people");
      const channelDetailsDialog = document.querySelector("#channel-details-dialog");
      const channelDetailsClose = document.querySelector("#channel-details-close");
      const channelMemberSearch = document.querySelector("#channel-member-search");
      const channelMemberCount = document.querySelector("#channel-member-count");
      const channelMemberList = document.querySelector("#channel-member-list");
      const channelDescriptionForm = document.querySelector("#channel-description-form");
      const channelDescriptionInput = document.querySelector("#channel-description-input");
      const channelDescriptionStatus = document.querySelector("#channel-description-status");
      const circleSelect = document.querySelector("#circle-select");
      const circleName = document.querySelector("#circle-name");
      const circleSlug = document.querySelector("#circle-slug");
      const channelMemberAdd = document.querySelector("#channel-member-add");
      const channelMember = document.querySelector("#channel-member");
      const addChannelMember = document.querySelector("#add-channel-member");
      const inviteChannelMember = document.querySelector("#invite-channel-member");
      const channelMemberStatus = document.querySelector("#channel-member-status");
      const invitationToken = document.querySelector("#invitation-token");
      const copyInvitation = document.querySelector("#copy-invitation");
      const createAgentAccessButton = document.querySelector("#create-agent-access");
      const copyAgentCredentialButton = document.querySelector("#copy-agent-credential");
      const revokeAgentAccessButton = document.querySelector("#revoke-agent-access");
      const agentCredential = document.querySelector("#agent-credential");
      const agentAccessNotice = document.querySelector("#agent-access-notice");
      const onboardingNotice = document.querySelector("#onboarding-notice");
      const createCircleButton = document.querySelector("#create-circle");
      const createCircleInvitationButton = document.querySelector("#create-invitation");
      const acceptInvitationButton = document.querySelector("#accept-invitation");
      const deleteCircleButton = document.querySelector("#delete-circle");
      const circleButtons = [createCircleButton, createCircleInvitationButton, acceptInvitationButton, deleteCircleButton];
      const exportButton = document.querySelector("#export-data");
      const processTitle = document.querySelector("#process-title");
      const processId = document.querySelector("#process-id");
      const processView = document.querySelector("#process-view");
      const processButtons = ["#enable-heart", "#start-process", "#refresh-process", "#inspect-process", "#process-yes", "#process-no"].map((id) => document.querySelector(id));
      const sidebar = document.querySelector("#sidebar-panel");
      const appMain = document.querySelector("main");
      const desktopSidebarToggle = document.querySelector("#desktop-sidebar-toggle");
      const desktopAdvancedEntry = document.querySelector("#desktop-advanced-entry");
      const statusEditor = document.querySelector("#status-editor");
      const notificationEditor = document.querySelector("#notification-editor");
      const currentStatusIcon = document.querySelector(".status-compact-icon");
      const currentStatusLabel = document.querySelector(".status-summary-label");
      const notificationSummaryLabel = document.querySelector(".notification-summary-label");
      const mobileNavigationToggle = document.querySelector("#mobile-navigation-toggle");
      const composerArea = document.querySelector(".composer-area");

      const connectionSupervisor = (() => {
        const state = {
          socket: null,
          socketHandoff: null,
          subscribedChannelId: null,
          recoveryPromise: null,
          reconnectTimer: null,
          reconnectAttempt: 0,
          heartbeatTimer: null,
          stableConnectionTimer: null
        };
        return Object.freeze({
          state,
          start() { connect(); },
          recover: recoverConnection,
          replaceAfterSessionRefresh: reconnectAfterSessionRefresh,
          scheduleReconnect,
          send: sendCommand
        });
      })();
      let lastBackgroundRecoveryAt = 0;
      let lastUserActivityAt = Date.now();
      let renderMode = "view";
      let requestNumber = 0;
      const browserSessionId = `browser-${crypto.randomUUID()}`;
      const sessionRefreshLeaseKey = "sproyt.session-refresh-lease.v1";
      const sessionRefreshBroadcast = typeof BroadcastChannel === "function" ? new BroadcastChannel("sproyt-session-refresh-v1") : null;
      let activeChannelId = null;
      let activeCircleId = null;
      let activeRootScope = "shared";
      let activeInboxKind = null;
      let managedCircleId = null;
      let reconnectScrollOffset = null;
      const activeConversationKey = "sproyt.active-channel.v1";
      const activeCircleKey = "sproyt.active-circle.v1";
      const circleChannelHistoryKey = "sproyt.active-channel-by-circle.v1";
      const channelDraftPrefix = "sproyt.channel-draft.v1.";
      const threadDraftPrefix = "sproyt.thread-draft.v1.";
      const linkedChannelId = new URL(window.location.href).searchParams.get("channel");
      let restoredChannelId = linkedChannelId;
      if (!restoredChannelId) try { restoredChannelId = window.localStorage.getItem(activeConversationKey); } catch (_) {}
      let restoredCircleId = null;
      try { restoredCircleId = window.localStorage.getItem(activeCircleKey); } catch (_) {}
      let lastChannelByCircle = {};
      try {
        const storedCircleChannels = JSON.parse(window.localStorage.getItem(circleChannelHistoryKey) || "{}");
        if (storedCircleChannels && typeof storedCircleChannels === "object" && !Array.isArray(storedCircleChannels)) {
          lastChannelByCircle = Object.fromEntries(Object.entries(storedCircleChannels)
            .filter(([circleId, channelId]) => typeof circleId === "string" && circleId.length <= 128 && typeof channelId === "string" && channelId.length <= 128));
        }
      } catch (_) {}
      let currentParticipantId = null;
      let requestedChannelSlug = "general";
      const timeline = [];
      const threadReplies = new Map();
      const threadRoots = new Map();
      const threadSummaries = new Map();
      const pendingThreadReplies = new Map();
      let activeThreadRootId = null;
      let pendingThreadToOpen = null;
      const seenMessageIds = new Set();
      const catchUpTargets = new Map();
      const pendingCommands = new Map();
      const pendingInvitationResponses = new Map();
      const pendingInvitationInspections = new Map();
      const pendingChannelInvitationRecipients = new Map();
      const pendingDirectInvitationMessages = new Map();
      const invitationInspectionCache = new Map();
      let latestChannelListRequestId = null;
      let latestCircleListRequestId = null;
      const pendingMessages = new Map();
      const historyRequestIds = new Set();
      const historyPageSize = 50;
      let historyHasMore = false;
      let historyLoading = false;
      let mermaidPromise = null;
      let knownChannels = [];
      let knownUsers = [];
      const knownCircleUsers = new Map();
      const knownChannelUsers = new Map();
      let knownMentions = [];
      let knownTasks = [];
      const knownCircles = new Map();
      let temporaryAgentId = null;
      let pendingMedia = [];
      const threadComposerStates = new Map();
      const messageReactions = new Map();
      const reactionEmojis = [...document.querySelectorAll("#message-emoji-options [data-emoji]")].map((button) => button.dataset.emoji);
      let mentionMatches = [];
      let selectedMentionIndex = 0;
      let activeMention = null;
      let composerHasFocus = false;
      let composerComposing = false;
      const statusDraft = { emoji: "", text: "", dirty: false };
      const usesDesktopComposerKeys = window.matchMedia("(any-hover: hover) and (any-pointer: fine)");

      const applicationStore = createApplicationStore();

      const sessionSupervisor = (() => {
        const state = {
          refreshTimer: null,
          refreshDueAt: 0,
          refreshPromise: null,
          refreshRejected: false,
          authenticationRecoveryPromise: null
        };
        return Object.freeze({
          state,
          start() {
            scheduleInitialSessionRefresh().catch(() => scheduleSessionRefresh(30));
          },
          schedule: scheduleSessionRefresh,
          refresh: refreshSession,
          recoverAuthentication
        });
      })();

      const serverEventMailbox = createServerEventMailbox({
        reduce: applicationStore.reduceServerEvent,
        deliver: renderServerEvent
      });

      function channelDraftKey(channelId) {
        return `${channelDraftPrefix}${channelId}`;
      }

      function persistActiveDraft() {
        if (!activeChannelId) return;
        try {
          const key = channelDraftKey(activeChannelId);
          if (bodyInput.value) window.localStorage.setItem(key, bodyInput.value);
          else window.localStorage.removeItem(key);
        } catch (_) {}
      }

      function restoreActiveDraft() {
        try { bodyInput.value = window.localStorage.getItem(channelDraftKey(activeChannelId)) || ""; }
        catch (_) { bodyInput.value = ""; }
        syncComposerState();
      }

      function threadDraftKey(channelId, rootId) {
        return `${threadDraftPrefix}${channelId}.${rootId}`;
      }

      function persistThreadDraft(rootId = activeThreadRootId, channelId = activeChannelId) {
        const state = threadComposerStates.get(rootId);
        if (!rootId || !channelId || !state) return;
        try {
          const key = threadDraftKey(channelId, rootId);
          if (state.draft) window.localStorage.setItem(key, state.draft);
          else window.localStorage.removeItem(key);
        } catch (_) {}
      }

      function restoreThreadDraft(rootId, channelId) {
        try { return window.localStorage.getItem(threadDraftKey(channelId, rootId)) || ""; }
        catch (_) { return ""; }
      }

      function clearThreadDraft(rootId, channelId) {
        if (!rootId || !channelId) return;
        try { window.localStorage.removeItem(threadDraftKey(channelId, rootId)); } catch (_) {}
      }

      function activeChannelMedia() {
        return pendingMedia.filter((media) => media.channel_id === activeChannelId);
      }

      function resizeComposer() {
        const styles = window.getComputedStyle(bodyInput);
        const minimum = Number.parseFloat(styles.minHeight);
        const maximum = Number.parseFloat(styles.maxHeight);
        bodyInput.style.height = "auto";
        const height = bodyInput.value.length === 0
          ? minimum
          : Math.min(bodyInput.scrollHeight, maximum);
        bodyInput.style.height = `${height}px`;
        bodyInput.style.overflowY = bodyInput.value.length > 0 && bodyInput.scrollHeight > maximum
          ? "auto"
          : "hidden";
      }

      function syncComposerState() {
        const expanded = composerHasFocus
          || bodyInput.value.length > 0
          || activeChannelMedia().length > 0
          || uploadStatus.textContent.length > 0
          || !mentionSuggestions.hidden;
        sendForm.classList.toggle("is-expanded", expanded);
        composerTools.hidden = !composerHasFocus;
        resizeComposer();
      }

      function closeComposerToolsAfterFocusLeaves() {
        window.setTimeout(() => {
          if (sendForm.contains(document.activeElement)) return;
          composerHasFocus = false;
          messageEmojiPicker.open = false;
          closeMentionSuggestions();
          syncComposerState();
        }, 0);
      }

      function setActiveCircle(circleId) {
        activeCircleId = circleId || null;
        if (activeCircleId) {
          activeRootScope = "circle";
          restoredCircleId = activeCircleId;
          try { window.localStorage.setItem(activeCircleKey, activeCircleId); } catch (_) {}
        }
      }

      function clearActiveCircle(circleId = null) {
        if (circleId && activeCircleId !== circleId && restoredCircleId !== circleId) return;
        activeCircleId = null;
        restoredCircleId = null;
        try { window.localStorage.removeItem(activeCircleKey); } catch (_) {}
      }

      function restoreActiveCircle() {
        const selected = [activeCircleId, restoredCircleId, circleSelect.value]
          .find((circleId) => circleId && knownCircles.has(circleId));
        const fallback = selected || knownCircles.keys().next().value || null;
        if (!fallback) {
          clearActiveCircle();
          circleSelect.value = "";
          return null;
        }
        setActiveCircle(fallback);
        circleSelect.value = fallback;
        return fallback;
      }

      function persistCircleChannelHistory() {
        try { window.localStorage.setItem(circleChannelHistoryKey, JSON.stringify(lastChannelByCircle)); } catch (_) {}
      }

      function rememberCircleChannel(channel) {
        if (!channel?.circle_id) return;
        lastChannelByCircle[channel.circle_id] = channel.id;
        persistCircleChannelHistory();
      }

      function forgetCircleChannel(circleId) {
        if (!circleId || !(circleId in lastChannelByCircle)) return;
        delete lastChannelByCircle[circleId];
        persistCircleChannelHistory();
      }

      function preferredCircleChannel(circleId, channels = knownChannels) {
        const available = channels.filter((channel) => channel.circle_id === circleId);
        const remembered = available.find((channel) => channel.id === lastChannelByCircle[circleId]);
        const primary = available.find((channel) => channel.name.trim().toLocaleLowerCase() === "prat"
          || channel.slug === scopedCircleChannelSlug(circleId, "prat"));
        return remembered || primary || available[0] || null;
      }

      function reportClientEvent(event) {
        const participant = new URL(window.location.href).searchParams.get("participant");
        const query = participant ? `?participant=${encodeURIComponent(participant)}` : "";
        fetch(`/api/v1/client-events${query}`, {
          method: "POST",
          credentials: "same-origin",
          cache: "no-store",
          keepalive: true,
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ event })
        }).catch(() => {});
      }

      function scheduleSessionRefresh(seconds) {
        if (sessionSupervisor.state.refreshTimer !== null) {
          window.clearTimeout(sessionSupervisor.state.refreshTimer);
        }
        const delay = Math.max(1, Number(seconds) || 1) * 1000;
        sessionSupervisor.state.refreshDueAt = Date.now() + delay;
        applicationStore.updateSession({ refreshDueAt: sessionSupervisor.state.refreshDueAt });
        sessionSupervisor.state.refreshTimer = window.setTimeout(
          () => refreshSession().catch(() => scheduleSessionRefresh(30)),
          delay
        );
      }

      sessionRefreshBroadcast?.addEventListener("message", (event) => {
        const seconds = Number(event.data?.refreshAfterSeconds);
        if (Number.isFinite(seconds) && seconds > 0) sessionSupervisor.schedule(seconds);
        if (event.data?.type === "session_rotated") connectionSupervisor.replaceAfterSessionRefresh();
      });

      function reconnectAfterSessionRefresh() {
        const currentSocket = connectionSupervisor.state.socket;
        if (connectionSupervisor.state.socketHandoff) return;
        if (!currentSocket || currentSocket.readyState === WebSocket.CLOSED || currentSocket.readyState === WebSocket.CLOSING) {
          connect(true);
          return;
        }
        if (currentSocket.readyState === WebSocket.OPEN) connect(true, currentSocket);
      }

      async function performSessionRefresh() {
        const showRefreshIndicator = document.visibilityState === "visible"
          && connectionSupervisor.state.socket?.readyState === WebSocket.OPEN;
        if (showRefreshIndicator) setConnectionStatus("Fornyar økta …");
        let response;
        try {
          response = await fetch("/auth/refresh", {
            method: "POST",
            credentials: "same-origin",
            headers: { "accept": "application/json" }
          });
        } catch (_) {
          reportClientEvent("session_refresh_failed");
          sessionSupervisor.state.refreshRejected = false;
          scheduleSessionRefresh(30);
          if (showRefreshIndicator) setConnectionStatus("Tilkopla");
          return false;
        }
        if (response.status === 401) {
          // The active WebSocket revalidates the session and redirects on a
          // real authentication expiry. A refresh token is optional at some
          // OIDC providers, so a failed proactive refresh must not create an
          // Authentik callback/reload loop while the session is still valid.
          reportClientEvent("session_refresh_failed");
          sessionSupervisor.state.refreshRejected = true;
          scheduleSessionRefresh(30);
          if (showRefreshIndicator) setConnectionStatus("Tilkopla");
          return false;
        }
        if (!response.ok) {
          reportClientEvent("session_refresh_failed");
          sessionSupervisor.state.refreshRejected = false;
          scheduleSessionRefresh(30);
          if (showRefreshIndicator) setConnectionStatus("Tilkopla");
          return false;
        }
        sessionSupervisor.state.refreshRejected = false;
        const result = await response.json();
        const verification = await fetch("/auth/session", {
          credentials: "same-origin",
          cache: "no-store",
          headers: { "accept": "application/json" }
        });
        if (!verification.ok) {
          reportClientEvent("session_refresh_failed");
          sessionSupervisor.state.refreshRejected = verification.status === 401;
          scheduleSessionRefresh(30);
          if (showRefreshIndicator) setConnectionStatus("Tilkopla");
          return false;
        }
        scheduleSessionRefresh(Number(result.refresh_after_seconds) || 300);
        sessionRefreshBroadcast?.postMessage({
          type: "session_rotated",
          refreshAfterSeconds: Number(result.refresh_after_seconds) || 300
        });
        reportClientEvent("session_refresh_succeeded");
        reconnectAfterSessionRefresh();
        return true;
      }

      async function refreshWithLocalStorageLease() {
        const now = Date.now();
        const lease = { owner: browserSessionId, expiresAt: now + 15000 };
        try {
          const current = JSON.parse(window.localStorage.getItem(sessionRefreshLeaseKey) || "null");
          if (current?.owner !== browserSessionId && Number(current?.expiresAt) > now) {
            scheduleSessionRefresh(Math.max(2, Math.ceil((current.expiresAt - now) / 1000)));
            return false;
          }
          window.localStorage.setItem(sessionRefreshLeaseKey, JSON.stringify(lease));
          const acquired = JSON.parse(window.localStorage.getItem(sessionRefreshLeaseKey) || "null");
          if (acquired?.owner !== browserSessionId) {
            scheduleSessionRefresh(5);
            return false;
          }
        } catch (_) {
          return performSessionRefresh();
        }
        try {
          return await performSessionRefresh();
        } finally {
          try {
            const current = JSON.parse(window.localStorage.getItem(sessionRefreshLeaseKey) || "null");
            if (current?.owner === browserSessionId) window.localStorage.removeItem(sessionRefreshLeaseKey);
          } catch (_) {}
        }
      }

      async function useCurrentSessionIfAnotherTabRenewed() {
        try {
          const response = await fetch("/auth/session", { credentials: "same-origin", cache: "no-store", headers: { "accept": "application/json" } });
          if (!response.ok) return false;
          const result = await response.json();
          const seconds = Number(result.refresh_after_seconds) || 300;
          scheduleSessionRefresh(seconds);
          return true;
        } catch (_) {
          return false;
        }
      }

      async function refreshSession(waitForLock = false) {
        if (sessionSupervisor.state.refreshPromise) return sessionSupervisor.state.refreshPromise;
        sessionSupervisor.state.refreshPromise = (async () => {
          if (navigator.locks) {
            const options = waitForLock ? {} : { ifAvailable: true };
            return navigator.locks.request("sproyt-session-refresh", options, async (lock) => {
              if (lock) {
                if (waitForLock && await useCurrentSessionIfAnotherTabRenewed()) return true;
                return performSessionRefresh();
              }
              scheduleSessionRefresh(30);
              return false;
            });
          }
          return refreshWithLocalStorageLease();
        })();
        try {
          return await sessionSupervisor.state.refreshPromise;
        } finally {
          sessionSupervisor.state.refreshPromise = null;
        }
      }

      async function scheduleInitialSessionRefresh() {
        const response = await fetch("/auth/session", {
          credentials: "same-origin",
          cache: "no-store",
          headers: { "accept": "application/json" }
        });
        if (!response.ok) {
          if (response.status === 401 && await refreshSession(true)) return;
          scheduleSessionRefresh(30);
          return;
        }
        const result = await response.json();
        scheduleSessionRefresh(Number(result.refresh_after_seconds) || 300);
      }

      async function recoverAuthentication() {
        if (sessionSupervisor.state.authenticationRecoveryPromise) {
          return sessionSupervisor.state.authenticationRecoveryPromise;
        }
        sessionSupervisor.state.authenticationRecoveryPromise = (async () => {
          setConnectionStatus("Fornyar økta …");
          const refreshed = await refreshSession(true);
          if (refreshed) {
            const currentSocket = connectionSupervisor.state.socket;
            if (!currentSocket || currentSocket.readyState === WebSocket.CLOSED || currentSocket.readyState === WebSocket.CLOSING) connect(true);
            return;
          }
          if (sessionSupervisor.state.refreshRejected) {
            // A second tab may have rotated the shared cookies while this tab
            // received a losing 401. Verify once more before any navigation.
            if (await useCurrentSessionIfAnotherTabRenewed()) {
              reconnectAfterSessionRefresh();
              return;
            }
            if (document.visibilityState === "visible" && Date.now() - lastUserActivityAt < 120_000) {
              setConnectionStatus("Økta må stadfestast – vi ventar så du ikkje mistar arbeidet ditt");
              scheduleSessionRefresh(30);
              return;
            }
            setConnectionStatus("Økta må stadfestast på nytt …");
            window.location.assign("/auth/login");
            return;
          }
          scheduleReconnect(1006, "ventar på nett for å fornye økta");
        })();
        try {
          return await sessionSupervisor.state.authenticationRecoveryPromise;
        } finally {
          sessionSupervisor.state.authenticationRecoveryPromise = null;
        }
      }

      async function recoverConnection(replaceOpenSocket = false) {
        if (connectionSupervisor.state.recoveryPromise) {
          return connectionSupervisor.state.recoveryPromise;
        }
        connectionSupervisor.state.recoveryPromise = (async () => {
          let response;
          try {
            response = await fetch("/auth/session", {
              credentials: "same-origin",
              cache: "no-store",
              headers: { "accept": "application/json" }
            });
          } catch (_) {
            scheduleReconnect(1006, "ventar på nett");
            return;
          }
          if (response.status === 401) {
            await recoverAuthentication();
            return;
          }
          if (!response.ok) {
            scheduleReconnect(response.status, "kunne ikkje kontrollere økta");
            return;
          }
          const result = await response.json();
          scheduleSessionRefresh(Number(result.refresh_after_seconds) || 300);
          const currentSocket = connectionSupervisor.state.socket;
          if (!currentSocket || currentSocket.readyState === WebSocket.CLOSED || currentSocket.readyState === WebSocket.CLOSING) {
            connect(true);
          } else if (replaceOpenSocket && currentSocket.readyState === WebSocket.OPEN) {
            connect(true, currentSocket);
          }
        })();
        try {
          return await connectionSupervisor.state.recoveryPromise;
        } finally {
          connectionSupervisor.state.recoveryPromise = null;
        }
      }

      function resumeAfterBackground() {
        if (document.visibilityState === "hidden") return;
        const now = Date.now();
        if (now - lastBackgroundRecoveryAt < 5_000) return;
        lastBackgroundRecoveryAt = now;
        connectionSupervisor.recover(false)
          .catch(() => connectionSupervisor.scheduleReconnect(1006, "kunne ikkje gjenopprette sambandet"));
      }

      function noteUserActivity() {
        lastUserActivityAt = Date.now();
      }

      window.addEventListener("pointerdown", noteUserActivity, { passive: true });
      window.addEventListener("keydown", noteUserActivity, { passive: true });
      window.addEventListener("input", noteUserActivity, { passive: true });

      window.addEventListener("pageshow", resumeAfterBackground);
      window.addEventListener("focus", resumeAfterBackground);
      window.addEventListener("online", resumeAfterBackground);
      window.addEventListener("pageshow", refreshVisibleInvitationCards);
      window.addEventListener("focus", refreshVisibleInvitationCards);

      function renderMediaPreviews() {
        mediaPreviews.replaceChildren(...activeChannelMedia().map((media) => {
          const item = document.createElement("span");
          item.className = "media-preview";
          const label = document.createElement("span");
          label.className = "media-preview-label";
          label.textContent = `${media.content_type.startsWith("video/") ? "🎬" : "🖼️"} ${media.original_filename}`;
          const remove = document.createElement("button");
          remove.type = "button";
          remove.className = "media-preview-remove";
          remove.textContent = "×";
          remove.setAttribute("aria-label", `Fjern ${media.original_filename}`);
          remove.addEventListener("click", () => {
            if (pendingMessages.size > 0) return;
            pendingMedia = pendingMedia.filter((candidate) => candidate.id !== media.id);
            renderMediaPreviews();
            setUploadStatus(`${media.original_filename} er fjerna.`);
            window.setTimeout(() => {
              if (uploadStatus.textContent === `${media.original_filename} er fjerna.`) setUploadStatus("");
            }, 2_000);
            bodyInput.focus({ preventScroll: true });
          });
          item.append(label, remove);
          return item;
        }));
        syncComposerState();
      }

      function setUploadStatus(message, kind = "progress") {
        uploadStatus.textContent = message;
        uploadStatus.dataset.kind = kind;
        uploadStatus.setAttribute("aria-live", kind === "error" ? "assertive" : "polite");
        syncComposerState();
      }

      async function uploadFailureMessage(response, filename) {
        let detail = "";
        try { detail = (await response.text()).trim(); } catch (_) {}
        const trace = response.headers.get("cf-ray") || response.headers.get("x-request-id");
        const reason = detail && !detail.startsWith("<") ? `: ${detail}` : "";
        const reference = trace ? ` Referanse: ${trace}.` : "";
        return `Opplasting av ${filename} feila (HTTP ${response.status})${reason}.${reference}`;
      }

      function postMedia(url, form, filename, setStatus = setUploadStatus) {
        return new Promise((resolve, reject) => {
          const request = new XMLHttpRequest();
          request.open("POST", url);
          request.withCredentials = true;
          request.setRequestHeader("accept", "application/json");
          request.upload.addEventListener("progress", (event) => {
            const progress = event.lengthComputable ? ` ${Math.min(100, Math.round(event.loaded * 100 / event.total))} %` : "";
            setStatus(`Lastar opp ${filename}${progress} …`);
          });
          request.upload.addEventListener("load", () => {
            setStatus(`Opplasting av ${filename} er ferdig. Behandlar fila …`);
          });
          request.addEventListener("load", () => resolve({
            status: request.status,
            ok: request.status >= 200 && request.status < 300,
            headers: { get: (name) => request.getResponseHeader(name) },
            text: async () => request.responseText,
            json: async () => JSON.parse(request.responseText)
          }));
          request.addEventListener("error", () => reject(new Error("Nettverkssambandet vart brote")));
          request.addEventListener("abort", () => reject(new Error("Opplastinga vart avbroten")));
          request.send(form);
        });
      }

      async function uploadMediaFiles(files) {
        if (!activeChannelId) return;
        for (const file of files) {
          if (!file.size || file.size > 35 * 1024 * 1024) {
            setUploadStatus(`${file.name || "Fila"} må vere mellom 1 byte og 35 MiB.`, "error");
            continue;
          }
          const form = new FormData();
          form.append("file", file, file.name || "clipboard-image.png");
          const filename = file.name || "bilete";
          setUploadStatus(`Gjer klar ${filename} (${(file.size / 1024 / 1024).toFixed(1)} MiB) …`);
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const url = `/api/v1/channels/${activeChannelId}/media${authQuery}`;
          let response;
          try {
            response = await postMedia(url, form, filename);
            if (response.status === 401 && await refreshSession(true)) {
              response = await postMedia(url, form, filename);
            }
          } catch (error) {
            reportClientEvent("upload_failed");
            const online = navigator.onLine ? "Nettlesaren fekk ikkje noko HTTP-svar frå tenesta" : "Eininga er fråkopla nettet";
            setUploadStatus(`Opplasting av ${file.name || "fila"} feila: ${online}. ${error?.message || "Ukjend nettverksfeil"}.`, "error");
            continue;
          }
          if (response.status === 401) {
            reportClientEvent("upload_failed");
            setUploadStatus("Opplasting feila (HTTP 401): Økta kunne ikkje fornyast. Logg inn på nytt.", "error");
            continue;
          }
          if (!response.ok) { reportClientEvent("upload_failed"); setUploadStatus(await uploadFailureMessage(response, file.name || "fila"), "error"); continue; }
          const result = await response.json();
          pendingMedia.push(result.media);
          renderMediaPreviews();
          reportClientEvent("upload_succeeded");
          setUploadStatus(`${file.name || "Fila"} er behandla og klar til å sendast.`, "success");
        }
        setConnected(connectionSupervisor.state.socket?.readyState === WebSocket.OPEN, "Tilkopla");
      }

      function threadComposerState(rootId = activeThreadRootId) {
        if (!rootId) return null;
        if (!threadComposerStates.has(rootId)) {
          threadComposerStates.set(rootId, { draft: "", media: [], status: "", statusKind: "progress", hasFocus: false, composing: false, uploadCount: 0 });
        }
        return threadComposerStates.get(rootId);
      }

      function hasPendingThreadReply(rootId = activeThreadRootId) {
        return [...pendingThreadReplies.values()].some((pending) => pending.rootId === rootId);
      }

      function resizeThreadComposer() {
        const styles = window.getComputedStyle(threadBody);
        const minimum = Number.parseFloat(styles.minHeight);
        const maximum = Number.parseFloat(styles.maxHeight);
        threadBody.style.height = "auto";
        const height = threadBody.value.length === 0 ? minimum : Math.min(threadBody.scrollHeight, maximum);
        threadBody.style.height = `${height}px`;
        threadBody.style.overflowY = threadBody.value.length > 0 && threadBody.scrollHeight > maximum ? "auto" : "hidden";
      }

      function syncThreadComposer() {
        const state = threadComposerState();
        if (!state) return;
        const expanded = state.hasFocus || threadBody.value.length > 0 || state.media.length > 0 || state.status.length > 0 || state.uploadCount > 0 || threadEmojiPicker.open;
        threadForm.classList.toggle("is-expanded", expanded);
        threadComposerTools.hidden = !state.hasFocus;
        const writable = Boolean(activeThreadRootId && activeChannelId && connectionSupervisor.state.subscribedChannelId === activeChannelId && connectionSupervisor.state.socket?.readyState === WebSocket.OPEN);
        threadBody.disabled = !writable;
        threadAttachMediaButton.disabled = !writable || state.uploadCount > 0;
        threadSendButton.disabled = !writable || state.uploadCount > 0 || hasPendingThreadReply();
        threadEmojiPicker.setAttribute("aria-disabled", String(!writable || state.uploadCount > 0));
        resizeThreadComposer();
      }

      function setThreadUploadStatus(message, kind = "progress", rootId = activeThreadRootId) {
        const state = threadComposerState(rootId);
        if (!state) return;
        state.status = message;
        state.statusKind = kind;
        if (rootId === activeThreadRootId) {
          threadUploadStatus.textContent = message;
          threadUploadStatus.dataset.kind = kind;
          threadUploadStatus.setAttribute("aria-live", kind === "error" ? "assertive" : "polite");
          syncThreadComposer();
        }
      }

      function renderThreadMediaPreviews() {
        const state = threadComposerState();
        if (!state) return;
        threadMediaPreviews.replaceChildren(...state.media.map((media) => {
          const item = document.createElement("span");
          item.className = "media-preview";
          const label = document.createElement("span");
          label.className = "media-preview-label";
          label.textContent = `${media.content_type.startsWith("video/") ? "🎬" : "🖼️"} ${media.original_filename}`;
          const remove = document.createElement("button");
          remove.type = "button";
          remove.className = "media-preview-remove";
          remove.textContent = "×";
          remove.setAttribute("aria-label", `Fjern ${media.original_filename}`);
          remove.addEventListener("click", () => {
            if (state.uploadCount > 0 || hasPendingThreadReply()) return;
            state.media = state.media.filter((candidate) => candidate.id !== media.id);
            setThreadUploadStatus(`${media.original_filename} er fjerna.`);
            renderThreadMediaPreviews();
            threadBody.focus({ preventScroll: true });
          });
          item.append(label, remove);
          return item;
        }));
        syncThreadComposer();
      }

      async function uploadThreadMediaFiles(files) {
        const channelId = activeChannelId;
        const rootId = activeThreadRootId;
        const state = threadComposerState(rootId);
        if (!channelId || !rootId || !state) return;
        for (const file of files) {
          if (!file.size || file.size > 35 * 1024 * 1024) {
            setThreadUploadStatus(`${file.name || "Fila"} må vere mellom 1 byte og 35 MiB.`, "error", rootId);
            continue;
          }
          state.uploadCount += 1;
          syncThreadComposer();
          const form = new FormData();
          form.append("file", file, file.name || "clipboard-image.png");
          const filename = file.name || "bilete";
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const url = `/api/v1/channels/${channelId}/media${authQuery}`;
          try {
            let response = await postMedia(url, form, filename, (message, kind) => setThreadUploadStatus(message, kind, rootId));
            if (response.status === 401 && await refreshSession(true)) response = await postMedia(url, form, filename, (message, kind) => setThreadUploadStatus(message, kind, rootId));
            if (response.status === 401) throw new Error("Økta kunne ikkje fornyast. Logg inn på nytt.");
            if (!response.ok) throw new Error(await uploadFailureMessage(response, filename));
            const result = await response.json();
            // The upload belongs to the channel and root that were active when it started.
            state.media.push({ ...result.media, channel_id: channelId, parent_message_id: rootId });
            reportClientEvent("upload_succeeded");
            setThreadUploadStatus(`${filename} er behandla og klar til å sendast.`, "success", rootId);
          } catch (error) {
            reportClientEvent("upload_failed");
            setThreadUploadStatus(`Opplasting av ${filename} feila: ${error?.message || "Ukjend feil"}`, "error", rootId);
          } finally {
            state.uploadCount = Math.max(0, state.uploadCount - 1);
            if (rootId === activeThreadRootId) syncThreadComposer();
          }
        }
      }

      attachMediaButton.addEventListener("click", () => {
        if (!attachMediaButton.disabled) mediaInput.click();
      });
      mediaInput.addEventListener("change", () => {
        uploadMediaFiles([...mediaInput.files]);
        mediaInput.value = "";
        bodyInput.focus();
      });
      bodyInput.addEventListener("paste", (event) => {
        const files = [...event.clipboardData.files].filter((file) => file.type.startsWith("image/") || file.type.startsWith("video/"));
        if (files.length) { event.preventDefault(); uploadMediaFiles(files); }
      });
      threadAttachMediaButton.addEventListener("click", () => {
        if (!threadAttachMediaButton.disabled) threadMediaInput.click();
      });
      threadMediaInput.addEventListener("change", () => {
        uploadThreadMediaFiles([...threadMediaInput.files]);
        threadMediaInput.value = "";
        threadBody.focus({ preventScroll: true });
      });
      threadBody.addEventListener("paste", (event) => {
        const files = [...event.clipboardData.files].filter((file) => file.type.startsWith("image/") || file.type.startsWith("video/"));
        if (files.length) { event.preventDefault(); uploadThreadMediaFiles(files); }
      });
      document.querySelector("#media-lightbox-close").addEventListener("click", () => mediaLightbox.close());
      document.querySelector("#thread-close").addEventListener("click", () => threadPanel.close());
      threadPanel.addEventListener("close", () => {
        persistThreadDraft();
        activeThreadRootId = null;
        threadEmojiPicker.open = false;
      });
      document.querySelector("#circle-channel-close").addEventListener("click", () => circleChannelDialog.close());
      circleAdminClose.addEventListener("click", () => circleAdminDialog.close());
      circleAdminDialog.addEventListener("close", () => {
        if (bottomCirclePanel.open) circleToolSettings.focus({ preventScroll: true });
      });
      document.querySelector("#direct-message-close").addEventListener("click", () => directMessageDialog.close());
      channelPeopleButton.addEventListener("click", () => openChannelDetails(false));
      connectionStatusToggle.addEventListener("click", () => {
        connectionStatusToggle.setAttribute("aria-expanded", String(connectionStatusToggle.getAttribute("aria-expanded") !== "true"));
      });
      document.addEventListener("pointerdown", (event) => {
        if (!event.target.closest(".connection-status")) connectionStatusToggle.setAttribute("aria-expanded", "false");
        if (threadPanel.open && !threadEmojiPicker.contains(event.target)) threadEmojiPicker.open = false;
      });
      channelDetailsClose.addEventListener("click", () => channelDetailsDialog.close());
      channelMemberSearch.addEventListener("input", () => {
        const channelId = channelDetailsDialog.dataset.channelId;
        if (channelId) renderChannelMembers(channelId);
      });
      channelDescriptionForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const channelId = channelDetailsDialog.dataset.channelId;
        if (!channelId) return;
        channelDescriptionStatus.textContent = "Lagrar …";
        sendCommand("update_channel_description", {
          channel_id: channelId,
          description: channelDescriptionInput.value
        });
      });
      circleChannelDialog.addEventListener("close", () => { managedCircleId = null; });
      circleChannelCreate.addEventListener("submit", (event) => {
        event.preventDefault();
        const name = managedChannelName.value.trim();
        if (!managedCircleId || !name) return;
        sendCommand("create_channel", {
          slug: scopedCircleChannelSlug(managedCircleId, name),
          name,
          kind: managedChannelKind.value,
          circle_id: managedCircleId
        });
      });
      leaveCircleButton.addEventListener("click", () => {
        const circle = knownCircles.get(managedCircleId);
        if (!circle || circle.role === "owner") return;
        if (!window.confirm(`Vil du forlate vennekretsen ${circle.name}? Du mistar tilgang til alle kanalane i kretsen.`)) return;
        sendCommand("leave_circle", { circle_id: managedCircleId });
      });
      threadForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const rootId = activeThreadRootId;
        const channelId = activeChannelId;
        const state = threadComposerState(rootId);
        const draft = threadBody.value.trim();
        const media = state?.media || [];
        const mediaTokens = media.map((item) => `[[media:${item.id}|${item.content_type}|${encodeURIComponent(item.original_filename)}]]`).join("\n");
        const body = [draft, mediaTokens].filter(Boolean).join("\n");
        if (!body || !rootId || !channelId || !state || state.uploadCount > 0) return;
        const requestId = sendCommand("send_message", {
          channel_id: channelId,
          parent_message_id: activeThreadRootId,
          body
        });
        if (!requestId) return;
        pendingThreadReplies.set(requestId, { rootId, channelId, body, draft, mediaIds: media.map((item) => item.id) });
        threadBody.value = "";
        state.draft = "";
        threadBody.readOnly = true;
        syncThreadComposer();
      });
      threadForm.addEventListener("focusin", () => {
        const state = threadComposerState();
        if (state) state.hasFocus = true;
        syncThreadComposer();
      });
      threadForm.addEventListener("focusout", () => window.setTimeout(() => {
        if (threadForm.contains(document.activeElement)) return;
        const state = threadComposerState();
        if (state) state.hasFocus = false;
        threadEmojiPicker.open = false;
        syncThreadComposer();
      }, 0));
      threadBody.addEventListener("input", () => {
        const state = threadComposerState();
        if (state) {
          state.draft = threadBody.value;
          persistThreadDraft();
        }
        syncThreadComposer();
      });
      threadBody.addEventListener("compositionstart", () => { const state = threadComposerState(); if (state) state.composing = true; });
      threadBody.addEventListener("compositionend", () => { const state = threadComposerState(); if (state) { state.composing = false; state.draft = threadBody.value; persistThreadDraft(); } syncThreadComposer(); });
      threadBody.addEventListener("keydown", (event) => {
        const state = threadComposerState();
        if (event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229 && !state?.composing && usesDesktopComposerKeys.matches) {
          event.preventDefault();
          threadForm.requestSubmit();
        }
      });
      mediaLightbox.addEventListener("click", (event) => {
        if (event.target === mediaLightbox) mediaLightbox.close();
      });
      mediaLightbox.addEventListener("close", () => {
        mediaLightboxImage.removeAttribute("src");
      });

      function mentionHandle(user) {
        return user.display_name.toLocaleLowerCase().replace(/[^\p{L}\p{N}_-]/gu, "");
      }

      function closeMentionSuggestions() {
        activeMention = null;
        mentionMatches = [];
        selectedMentionIndex = 0;
        mentionSuggestions.hidden = true;
        bodyInput.setAttribute("aria-expanded", "false");
        bodyInput.removeAttribute("aria-activedescendant");
        syncComposerState();
      }

      function mentionCandidates() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        return channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers;
      }

      function selectMention(index) {
        const user = mentionMatches[index];
        if (!user || !activeMention) return;
        const replacement = `@${mentionHandle(user)} `;
        bodyInput.setRangeText(replacement, activeMention.start, activeMention.end, "end");
        closeMentionSuggestions();
        bodyInput.focus();
      }

      function renderMentionSuggestions() {
        mentionSuggestions.replaceChildren(...mentionMatches.map((user, index) => {
          const button = document.createElement("button");
          button.type = "button";
          button.id = `mention-option-${user.id}`;
          button.setAttribute("role", "option");
          button.setAttribute("aria-selected", String(index === selectedMentionIndex));
          const name = document.createElement("span");
          name.textContent = user.display_name;
          const handle = document.createElement("small");
          handle.textContent = `@${mentionHandle(user)}`;
          button.append(name, handle);
          button.addEventListener("pointerdown", (event) => event.preventDefault());
          button.addEventListener("click", () => selectMention(index));
          return button;
        }));
        mentionSuggestions.hidden = mentionMatches.length === 0;
        bodyInput.setAttribute("aria-expanded", String(mentionMatches.length > 0));
        if (mentionMatches.length > 0) {
          bodyInput.setAttribute("aria-activedescendant", `mention-option-${mentionMatches[selectedMentionIndex].id}`);
        } else {
          bodyInput.removeAttribute("aria-activedescendant");
        }
        syncComposerState();
      }

      function updateMentionSuggestions() {
        const caret = bodyInput.selectionStart;
        if (caret === null || bodyInput.selectionEnd !== caret) {
          closeMentionSuggestions();
          return;
        }
        const match = bodyInput.value.slice(0, caret).match(/(?:^|\s)@([\p{L}\p{N}_-]*)$/u);
        if (!match) {
          closeMentionSuggestions();
          return;
        }
        const query = match[1].toLocaleLowerCase();
        activeMention = { start: caret - query.length - 1, end: caret };
        mentionMatches = mentionCandidates()
          .filter((user) => mentionHandle(user).startsWith(query))
          .sort((left, right) => left.display_name.localeCompare(right.display_name));
        selectedMentionIndex = Math.min(selectedMentionIndex, Math.max(0, mentionMatches.length - 1));
        renderMentionSuggestions();
      }

      bodyInput.addEventListener("input", () => {
        persistActiveDraft();
        updateMentionSuggestions();
        syncComposerState();
      });
      bodyInput.addEventListener("click", updateMentionSuggestions);
      sendForm.addEventListener("focusin", () => { composerHasFocus = true; syncComposerState(); });
      sendForm.addEventListener("focusout", closeComposerToolsAfterFocusLeaves);
      document.addEventListener("pointerdown", (event) => {
        if (sendForm.contains(event.target)) return;
        composerHasFocus = false;
        messageEmojiPicker.open = false;
        closeMentionSuggestions();
        syncComposerState();
      });
      bodyInput.addEventListener("compositionstart", () => { composerComposing = true; syncComposerState(); });
      bodyInput.addEventListener("compositionend", () => { composerComposing = false; persistActiveDraft(); updateMentionSuggestions(); syncComposerState(); });
      bodyInput.addEventListener("keydown", (event) => {
        if (!mentionSuggestions.hidden && mentionMatches.length > 0 && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
          event.preventDefault();
          const direction = event.key === "ArrowDown" ? 1 : -1;
          selectedMentionIndex = (selectedMentionIndex + direction + mentionMatches.length) % mentionMatches.length;
          renderMentionSuggestions();
        } else if (!mentionSuggestions.hidden && mentionMatches.length > 0 && (event.key === "Enter" || event.key === "Tab")) {
          event.preventDefault();
          selectMention(selectedMentionIndex);
        } else if (!mentionSuggestions.hidden && mentionMatches.length > 0 && event.key === "Escape") {
          event.preventDefault();
          closeMentionSuggestions();
        } else if (event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229 && !composerComposing && usesDesktopComposerKeys.matches) {
          event.preventDefault();
          sendForm.requestSubmit();
        }
      });

      connectForm.addEventListener("submit", (event) => {
        event.preventDefault();
        connectionSupervisor.start();
      });

      sendForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const draft = bodyInput.value.trim();
        const channelMedia = pendingMedia.filter((media) => media.channel_id === activeChannelId);
        const mediaTokens = channelMedia.map((media) => `[[media:${media.id}|${media.content_type}|${encodeURIComponent(media.original_filename)}]]`).join("\n");
        const body = [draft, mediaTokens].filter(Boolean).join("\n");
        if (!connectionSupervisor.state.socket || connectionSupervisor.state.socket.readyState !== WebSocket.OPEN || !activeChannelId || body.length === 0) {
          return;
        }
        if (connectionSupervisor.state.subscribedChannelId !== activeChannelId) return;
        const requestId = sendCommand("send_message", { channel_id: activeChannelId, body });
        if (!requestId) return;
        pendingMessages.set(requestId, { body, draft, mediaIds: channelMedia.map((media) => media.id), channelId: activeChannelId });
        bodyInput.value = "";
        persistActiveDraft();
        closeMentionSuggestions();
        bodyInput.readOnly = true;
        syncComposerState();
        setConnected(true, "Sender meldinga …");
      });

      function insertEmoji(input, emoji) {
        const start = input.selectionStart ?? input.value.length;
        const end = input.selectionEnd ?? start;
        input.setRangeText(emoji, start, end, "end");
        if (input === bodyInput) { persistActiveDraft(); syncComposerState(); }
        if (input === threadBody) { const state = threadComposerState(); if (state) { state.draft = threadBody.value; persistThreadDraft(); } syncThreadComposer(); }
        input.focus();
      }

      document.querySelectorAll("#message-emoji-options [data-emoji]").forEach((button) => {
        button.addEventListener("click", () => {
          insertEmoji(bodyInput, button.dataset.emoji);
          messageEmojiPicker.open = false;
        });
      });
      document.querySelectorAll("#thread-emoji-options [data-emoji]").forEach((button) => {
        button.addEventListener("click", () => {
          insertEmoji(threadBody, button.dataset.emoji);
          threadEmojiPicker.open = false;
          syncThreadComposer();
        });
      });
      document.querySelectorAll("#status-emoji-options [data-emoji]").forEach((button) => {
        button.addEventListener("click", () => {
          statusEmoji.value = button.dataset.emoji;
          statusDraft.emoji = statusEmoji.value;
          statusDraft.text = statusText.value;
          statusDraft.dirty = true;
          document.querySelectorAll("#status-emoji-options [data-emoji]").forEach((option) => {
            option.setAttribute("aria-pressed", String(option === button));
          });
          statusText.focus();
        });
      });
      [statusEmoji, statusText].forEach((input) => {
        input.addEventListener("input", () => {
          statusDraft.emoji = statusEmoji.value;
          statusDraft.text = statusText.value;
          statusDraft.dirty = true;
        });
      });
      document.querySelector("#save-status").addEventListener("click", () => {
        statusDraft.emoji = statusEmoji.value;
        statusDraft.text = statusText.value;
        statusDraft.dirty = true;
        sendCommand("set_status", { text: statusDraft.text, emoji: statusDraft.emoji, expires_at: null });
      });
      document.querySelector("#clear-status").addEventListener("click", () => {
        statusText.value = "";
        statusEmoji.value = "";
        statusDraft.text = "";
        statusDraft.emoji = "";
        statusDraft.dirty = true;
        sendCommand("set_status", { text: "", emoji: "", expires_at: null });
      });

      function vapidKeyBytes(value) {
        const padding = "=".repeat((4 - value.length % 4) % 4);
        const raw = atob((value + padding).replace(/-/g, "+").replace(/_/g, "/"));
        return Uint8Array.from(raw, (character) => character.charCodeAt(0));
      }

      async function notificationRequest(path, options = {}) {
        const participant = new URL(window.location.href).searchParams.get("participant");
        const separator = path.includes("?") ? "&" : "?";
        const url = participant ? `${path}${separator}participant=${encodeURIComponent(participant)}` : path;
        let response = await fetch(url, { credentials: "same-origin", cache: "no-store", ...options });
        if (response.status === 401 && await refreshSession(true)) {
          response = await fetch(url, { credentials: "same-origin", cache: "no-store", ...options });
        }
        return response;
      }

      async function loadNotificationSettings() {
        try {
          const response = await notificationRequest("/api/v1/me/notifications", { headers: { accept: "application/json" } });
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const settings = await response.json();
          notificationMode.value = settings.preferences.mode;
          notificationDirect.checked = settings.preferences.direct_messages;
          notificationMentions.checked = settings.preferences.mentions;
          const notificationLabel = settings.preferences.mode === "muted" ? "Varsel: ingen" : settings.preferences.mode === "weekly" ? "Varsel: kvar veke" : "Varsel: direkte";
          notificationSummaryLabel.textContent = notificationLabel;
          notificationSummary.setAttribute("aria-label", notificationLabel);
          notificationSummary.title = notificationLabel;
          notificationSummary.dataset.tooltip = notificationLabel;
          enableNotifications.disabled = !settings.enabled || !("PushManager" in window) || !("Notification" in window) || Notification.permission === "denied";
          enableNotifications.dataset.publicKey = settings.public_key || "";
          notificationNotice.textContent = !settings.enabled ? "Push er ikkje konfigurert på serveren enno." : settings.subscriptions ? `${settings.subscriptions} eining(ar) tek imot varsel.` : "Varsel er ikkje slått på på denne eininga.";
        } catch (error) {
          notificationNotice.textContent = `Kunne ikkje hente varselinnstillingar: ${error.message}`;
        }
      }

      document.querySelector("#save-notifications").addEventListener("click", async () => {
        const response = await notificationRequest("/api/v1/me/notifications", {
          method: "PUT",
          headers: { "content-type": "application/json", accept: "application/json" },
          body: JSON.stringify({ mode: notificationMode.value, direct_messages: notificationDirect.checked, mentions: notificationMentions.checked, weekly_weekday: 1 })
        });
        notificationNotice.textContent = response.ok ? "Varselinnstillingane er lagra." : `Kunne ikkje lagre (HTTP ${response.status}).`;
        if (response.ok) loadNotificationSettings();
      });

      enableNotifications.addEventListener("click", async () => {
        try {
          const registration = await serviceWorkerReady;
          if (!registration) throw new Error("Service worker er ikkje tilgjengeleg");
          const permission = await Notification.requestPermission();
          if (permission !== "granted") throw new Error("Varsel vart ikkje tillate");
          let subscription = await registration.pushManager.getSubscription();
          if (!subscription) {
            subscription = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: vapidKeyBytes(enableNotifications.dataset.publicKey) });
          }
          const response = await notificationRequest("/api/v1/me/push-subscriptions", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify(subscription.toJSON())
          });
          if (!response.ok) throw new Error(`serveren svarte HTTP ${response.status}`);
          notificationNotice.textContent = "Varsel er slått på på denne eininga.";
          loadNotificationSettings();
        } catch (error) {
          notificationNotice.textContent = `Kunne ikkje slå på varsel: ${error.message}`;
        }
      });

      loadNotificationSettings();

      directUser.addEventListener("change", () => {
        openDirect.disabled = !directUser.value;
        directMessageStatus.textContent = "";
      });
      openDirect.addEventListener("click", () => {
        if (!directUser.value) return;
        const selectedName = directUser.options[directUser.selectedIndex]?.textContent || "personen";
        directMessageStatus.textContent = `Opnar samtale med ${selectedName} …`;
        openDirect.disabled = true;
        if (!sendCommand("open_direct_channel", { user_id: directUser.value })) {
          directMessageStatus.textContent = "Sprøyt er ikkje tilkopla. Vent litt og prøv igjen.";
          openDirect.disabled = false;
        }
      });
      document.querySelector("#show-unread").addEventListener("click", () => showInbox("unread"));
      document.querySelector("#show-mentions").addEventListener("click", () => showInbox("mentions"));
      document.querySelector("#show-tasks").addEventListener("click", () => showInbox("tasks"));

      const desktopSidebarStorageKey = "sproyt.desktop-sidebar-collapsed.v1";
      const compactDesktopViewport = window.matchMedia("(min-width: 641px) and (max-width: 900px)");
      function sidebarIsCollapsed() {
        return sidebar.classList.contains("desktop-collapsed");
      }
      function setDesktopSidebarCollapsed(collapsed, persist = true) {
        const effectiveCollapsed = collapsed;
        sidebar.classList.toggle("desktop-collapsed", effectiveCollapsed);
        sidebar.classList.toggle("desktop-expanded", !effectiveCollapsed);
        appMain.classList.toggle("desktop-sidebar-collapsed", effectiveCollapsed);
        appMain.classList.toggle("desktop-sidebar-expanded", !effectiveCollapsed);
        desktopSidebarToggle.setAttribute("aria-expanded", String(!effectiveCollapsed));
        const toggleLabel = effectiveCollapsed ? "Utvid menyen" : "Kollaps menyen";
        desktopSidebarToggle.setAttribute("aria-label", toggleLabel);
        desktopSidebarToggle.title = toggleLabel;
        desktopSidebarToggle.dataset.tooltip = toggleLabel;
        desktopSidebarToggle.textContent = effectiveCollapsed ? "›" : "‹";
        if (persist) {
          storedDesktopSidebarCollapsed = collapsed;
          try { window.localStorage.setItem(desktopSidebarStorageKey, String(collapsed)); } catch (_) {}
        }
      }
      let storedDesktopSidebarCollapsed = false;
      try { storedDesktopSidebarCollapsed = window.localStorage.getItem(desktopSidebarStorageKey) === "true"; } catch (_) {}
      setDesktopSidebarCollapsed(storedDesktopSidebarCollapsed || compactDesktopViewport.matches, false);
      desktopSidebarToggle.addEventListener("click", () => setDesktopSidebarCollapsed(!sidebarIsCollapsed()));
      compactDesktopViewport.addEventListener("change", () => setDesktopSidebarCollapsed(compactDesktopViewport.matches || storedDesktopSidebarCollapsed, false));
      function expandDesktopSidebarAndFocus(control) {
        if (!sidebarIsCollapsed()) return false;
        setDesktopSidebarCollapsed(false, false);
        control.open = true;
        window.requestAnimationFrame(() => control.querySelector("summary")?.focus());
        return true;
      }
      statusEditor.addEventListener("click", (event) => {
        if (event.target.closest("summary") && expandDesktopSidebarAndFocus(statusEditor)) event.preventDefault();
      });
      notificationEditor.addEventListener("click", (event) => {
        if (event.target.closest("summary") && expandDesktopSidebarAndFocus(notificationEditor)) event.preventDefault();
      });
      desktopAdvancedEntry?.addEventListener("click", () => {
        setDesktopSidebarCollapsed(false, false);
        window.requestAnimationFrame(() => {
          const control = document.querySelector(".advanced-tools button:not([disabled]), .advanced-tools input:not([disabled])");
          if (control) control.focus();
          else {
            processTitle.tabIndex = -1;
            processTitle.focus();
          }
        });
      });

      viewModeToggle.addEventListener("click", () => setRenderMode(renderMode === "raw" ? "view" : "raw"));
      function setMobileNavigationOpen(open, restoreFocus = false) {
        if (open) {
          bottomChannelPanel.open = false;
          bottomCirclePanel.open = false;
        }
        sidebar.classList.toggle("mobile-open", open);
        if (open) {
          sidebar.setAttribute("role", "dialog");
          sidebar.setAttribute("aria-modal", "true");
          sidebar.setAttribute("aria-label", "Sprøyt-meny");
        } else {
          sidebar.removeAttribute("role");
          sidebar.removeAttribute("aria-modal");
          sidebar.removeAttribute("aria-label");
        }
        messagesEl.inert = open;
        composerArea.inert = open;
        mobileNavigationToggle.setAttribute("aria-expanded", String(open));
        if (open) {
          const firstControl = sidebar.querySelector("button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary");
          firstControl?.focus();
        }
        if (restoreFocus) mobileNavigationToggle.focus();
      }
      mobileNavigationToggle.addEventListener("click", () => {
        setMobileNavigationOpen(!sidebar.classList.contains("mobile-open"));
      });
      bottomChannelPanel.addEventListener("toggle", () => {
        if (bottomChannelPanel.open) bottomCirclePanel.open = false;
      });
      bottomCirclePanel.addEventListener("toggle", () => {
        if (bottomCirclePanel.open) {
          bottomChannelPanel.open = false;
          renderBottomNavigation();
        }
      });
      circleToolDirect.addEventListener("click", () => activateRootScope("direct"));
      circleToolShared.addEventListener("click", () => activateRootScope("shared"));
      circleToolSettings.addEventListener("click", () => {
        updateOnboardingButtons();
        if (!circleAdminDialog.open) circleAdminDialog.showModal();
        window.setTimeout(() => circleAdminClose.focus(), 0);
      });
      document.addEventListener("pointerdown", (event) => {
        if (circleAdminDialog.open) return;
        if (bottomNavigation.contains(event.target)) return;
        bottomChannelPanel.open = false;
        bottomCirclePanel.open = false;
      });
      document.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && threadEmojiPicker.open) {
          event.preventDefault();
          threadEmojiPicker.open = false;
          threadEmojiPicker.querySelector("summary")?.focus({ preventScroll: true });
          return;
        }
        if (event.key === "Escape" && threadPanel.open) {
          const threadReactionPicker = threadMessages.querySelector(".reaction-picker[open]");
          if (threadReactionPicker) {
            event.preventDefault();
            threadReactionPicker.open = false;
            threadReactionPicker.closest(".message")?.classList.remove("reaction-picker-requested");
            threadReactionPicker.querySelector("summary")?.focus({ preventScroll: true });
            return;
          }
        }
        if (event.key === "Escape" && (circleChannelDialog.open || circleAdminDialog.open || threadPanel.open || mediaLightbox.open)) {
          return;
        }
        if (event.key === "Escape" && connectionStatusToggle.getAttribute("aria-expanded") === "true") {
          event.preventDefault();
          connectionStatusToggle.setAttribute("aria-expanded", "false");
          connectionStatusToggle.focus();
          return;
        }
        if (event.key === "Escape") {
          const reactionPicker = messagesEl.querySelector(".reaction-picker[open]");
          if (reactionPicker) {
            event.preventDefault();
            reactionPicker.open = false;
            reactionPicker.closest(".message")?.classList.remove("reaction-picker-requested");
            reactionPicker.querySelector("summary")?.focus({ preventScroll: true });
            return;
          }
          const messageMenu = messagesEl.querySelector(".message-menu[open]");
          if (messageMenu) {
            event.preventDefault();
            messageMenu.open = false;
            messageMenu.querySelector("summary")?.focus({ preventScroll: true });
            return;
          }
        }
        if (event.key === "Tab" && sidebar.classList.contains("mobile-open")) {
          const controls = Array.from(sidebar.querySelectorAll("button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary"))
            .filter((control) => !control.hidden && control.offsetParent !== null);
          if (controls.length > 0) {
            const first = controls[0];
            const last = controls[controls.length - 1];
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first.focus();
            }
          }
        }
        if (event.key === "Escape" && sidebar.classList.contains("mobile-open")) {
          event.preventDefault();
          setMobileNavigationOpen(false, true);
          return;
        }
        if (event.key === "Escape" && bottomChannelPanel.open) {
          event.preventDefault();
          closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
          return;
        }
        if (event.key === "Escape" && bottomCirclePanel.open) {
          event.preventDefault();
          closeBottomNavigation(bottomCirclePanel, bottomCircleToggle);
          return;
        }
      });
      createCircleButton.addEventListener("click", () => sendCommand("create_circle", {
        name: circleName.value.trim(), slug: slugify(circleSlug.value || circleName.value)
      }));
      circleName.addEventListener("input", updateOnboardingButtons);
      invitationToken.addEventListener("input", updateOnboardingButtons);
      circleSelect.addEventListener("change", updateOnboardingButtons);
      circleSelect.addEventListener("change", () => {
        if (circleSelect.value) {
          setActiveCircle(circleSelect.value);
          sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
        } else {
          clearActiveCircle();
        }
      });
      channelMember.addEventListener("change", updateOnboardingButtons);
      inviteChannelMember.addEventListener("click", () => {
        const channel = knownChannels.find((item) => item.id === channelDetailsDialog.dataset.channelId);
        if (!channel?.circle_id || !channelMember.value) return;
        channelMemberStatus.textContent = "Lagar invitasjonen …";
        const requestId = sendCommand("create_invitation", {
          target: { type: "channel", circle_id: channel.circle_id, channel_id: channel.id }
        });
        if (requestId) pendingChannelInvitationRecipients.set(requestId, channelMember.value);
      });
      addChannelMember.addEventListener("click", () => {
        const channelId = channelDetailsDialog.dataset.channelId;
        if (channelId && channelMember.value) {
          channelMemberStatus.textContent = "Legg brukaren til …";
          sendCommand("add_channel_member", { channel_id: channelId, user_id: channelMember.value });
        }
      });
      createCircleInvitationButton.addEventListener("click", () => {
        if (circleSelect.value) sendCommand("create_invitation", { target: { type: "circle", circle_id: circleSelect.value } });
      });
      acceptInvitationButton.addEventListener("click", () => {
        const token = invitationValueToToken(invitationToken.value);
        if (token) {
          onboardingNotice.textContent = "Kontrollerer invitasjonen …";
          sendCommand("accept_invitation", { token });
        }
      });
      copyInvitation.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(invitationToken.value);
          onboardingNotice.textContent = "Invitasjonslenkja er kopiert. Send henne til venen du vil invitere.";
        } catch (_) {
          invitationToken.focus();
          invitationToken.select();
          onboardingNotice.textContent = "Kopier den markerte lenkja og send henne til venen din.";
        }
      });
      createAgentAccessButton.addEventListener("click", createTemporaryAgentAccess);
      copyAgentCredentialButton.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(agentCredential.value);
          agentAccessNotice.textContent = "Credential er kopiert. Handsam han som eit passord.";
        } catch (_) {
          agentCredential.hidden = false;
          agentCredential.focus();
          agentCredential.select();
          agentAccessNotice.textContent = "Kopier den markerte credentialen manuelt.";
        }
      });
      revokeAgentAccessButton.addEventListener("click", revokeTemporaryAgentAccess);
      deleteCircleButton.addEventListener("click", () => {
        if (!circleSelect.value) return;
        const selected = circleSelect.options[circleSelect.selectedIndex]?.textContent || "denne vennekretsen";
        if (window.confirm(`Slett ${selected} og all chat- og prosesshistorikk permanent?`)) {
          sendCommand("delete_circle", { circle_id: circleSelect.value });
        }
      });
      messagesEl.addEventListener("scroll", () => {
        if (messagesEl.scrollTop <= 80) loadOlderHistory();
      }, { passive: true });
      exportButton.addEventListener("click", async () => {
        try {
          const response = await fetch("/api/v1/me/export", {
            credentials: "same-origin"
          });
          if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
          const blob = await response.blob();
          const url = URL.createObjectURL(blob);
          const link = document.createElement("a");
          link.href = url;
          link.download = "sproyt-export.json";
          link.click();
          URL.revokeObjectURL(url);
          pushSystem("Dataeksporten er laga.");
        } catch (error) {
          pushSystem(`Kunne ikkje eksportere data: ${error.message}`);
        }
      });
      processButtons[0].addEventListener("click", () => setHeartFeature(true));
      processButtons[1].addEventListener("click", startEventPlanning);
      processButtons[2].addEventListener("click", refreshProcess);
      processButtons[3].addEventListener("click", inspectProcess);
      processButtons[4].addEventListener("click", () => answerProcess("yes"));
      processButtons[5].addEventListener("click", () => answerProcess("no"));

      function slugify(value) {
        return value.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
      }

      function scopedCircleChannelSlug(circleId, value) {
        const scope = circleId.replace(/-/g, "");
        const base = slugify(value).replace(/^-+|-+$/g, "") || "kanal";
        return `${scope}-${base.slice(0, 47)}`;
      }

      function invitationValueToToken(value) {
        const candidate = value.trim();
        if (!candidate) return "";
        const meta = candidate.match(/^\[\[invite:([A-Za-z0-9_-]{32,128})\]\]$/);
        if (meta) return meta[1];
        try {
          const url = new URL(candidate, window.location.origin);
          return url.searchParams.get("invite") || candidate;
        } catch (_) {
          return candidate;
        }
      }

      function connect(silent = false, previousSocket = null) {
        if (connectionSupervisor.state.reconnectTimer !== null) {
          window.clearTimeout(connectionSupervisor.state.reconnectTimer);
          connectionSupervisor.state.reconnectTimer = null;
        }
        if (!previousSocket && connectionSupervisor.state.heartbeatTimer !== null) {
          window.clearInterval(connectionSupervisor.state.heartbeatTimer);
          connectionSupervisor.state.heartbeatTimer = null;
        }
        if (!previousSocket && connectionSupervisor.state.stableConnectionTimer !== null) {
          window.clearTimeout(connectionSupervisor.state.stableConnectionTimer);
          connectionSupervisor.state.stableConnectionTimer = null;
        }
        const currentSocket = connectionSupervisor.state.socket;
        if (!previousSocket && currentSocket && (currentSocket.readyState === WebSocket.OPEN || currentSocket.readyState === WebSocket.CONNECTING)) {
          return;
        }

        catchUpTargets.clear();
        if (activeChannelId && messagesEl.childElementCount > 0) {
          reconnectScrollOffset = Math.max(0, messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight);
        }
        if (!activeChannelId) {
          requestedChannelSlug = (channelInput.value.trim() || "")
            .toLowerCase()
            .replace(/[^a-z0-9_-]+/g, "-");
        }
        const protocol = window.location.protocol === "https:" ? "wss" : "ws";
        const websocketUrl = new URL(`${protocol}://${window.location.host}/ws`);
        const developmentParticipant = new URLSearchParams(window.location.search).get("participant");
        if (developmentParticipant) websocketUrl.searchParams.set("participant", developmentParticipant);
        const nextSocket = new WebSocket(websocketUrl);
        if (!previousSocket) {
          connectionSupervisor.state.socket = nextSocket;
          connectionSupervisor.state.subscribedChannelId = null;
        }
        if (!silent) setConnected(false, "Koplar til ...");

        nextSocket.addEventListener("open", () => {
          if (previousSocket && connectionSupervisor.state.socket !== previousSocket) {
            nextSocket.close(4000, "superseded session refresh");
            return;
          }
          if (!previousSocket && connectionSupervisor.state.socket !== nextSocket) return;
          connectionSupervisor.state.socket = nextSocket;
          if (previousSocket) {
            const handoff = { previousSocket, nextSocket, timeoutId: null };
            handoff.timeoutId = window.setTimeout(() => {
              if (connectionSupervisor.state.socketHandoff !== handoff) return;
              connectionSupervisor.state.socketHandoff = null;
              if (previousSocket.readyState === WebSocket.OPEN) {
                connectionSupervisor.state.socket = previousSocket;
                connectionSupervisor.state.subscribedChannelId = activeChannelId;
                setConnected(true, "Tilkopla");
                nextSocket.close(4001, "session handoff timed out");
                scheduleSessionRefresh(30);
                return;
              }
              nextSocket.close(4001, "session handoff timed out");
              connect(true);
            }, 10_000);
            connectionSupervisor.state.socketHandoff = handoff;
            if (activeChannelId) {
              setConnectionStatus("Gjenopprettar samtalen …");
            }
          }
          reportClientEvent("websocket_connected");
          if (connectionSupervisor.state.heartbeatTimer !== null) {
            window.clearInterval(connectionSupervisor.state.heartbeatTimer);
          }
          setConnected(true, "Tilkopla");
          connectionSupervisor.state.stableConnectionTimer = window.setTimeout(() => {
            if (connectionSupervisor.state.socket === nextSocket && nextSocket.readyState === WebSocket.OPEN) {
              connectionSupervisor.state.reconnectAttempt = 0;
            }
          }, 10_000);
          sendCommand("hello");
          sendCommand("list_users");
          sendCommand("list_my_channels");
          sendCommand("list_my_circles");
          sendCommand("list_mentions");
          sendCommand("list_tasks");
          if (activeChannelId) sendCommand("subscribe_channel", { channel_id: activeChannelId });
          connectionSupervisor.state.heartbeatTimer = window.setInterval(() => {
            sendCommand("ping");
          }, 20_000);
          if (previousSocket && !activeChannelId) finishSocketHandoff(nextSocket);
        });

        nextSocket.addEventListener("message", (event) => {
          if (connectionSupervisor.state.socket !== nextSocket) return;
          serverEventMailbox.enqueue(JSON.parse(event.data));
        });

        nextSocket.addEventListener("close", (event) => {
          if (connectionSupervisor.state.socketHandoff?.nextSocket === nextSocket) {
            const handoff = connectionSupervisor.state.socketHandoff;
            const fallbackSocket = handoff.previousSocket;
            if (handoff.timeoutId !== null) window.clearTimeout(handoff.timeoutId);
            connectionSupervisor.state.socketHandoff = null;
            if (fallbackSocket.readyState === WebSocket.OPEN) {
              connectionSupervisor.state.socket = fallbackSocket;
              connectionSupervisor.state.subscribedChannelId = activeChannelId;
              setConnected(true, "Tilkopla");
              scheduleSessionRefresh(30);
              return;
            }
          }
          if (previousSocket && connectionSupervisor.state.socket === previousSocket) {
            scheduleSessionRefresh(30);
            return;
          }
          if (connectionSupervisor.state.socket !== nextSocket) return;
          reportClientEvent("websocket_disconnected");
          connectionSupervisor.state.subscribedChannelId = null;
          for (const requestId of pendingMessages.keys()) {
            failPendingMessage(requestId, "sambandet vart brote; kontroller samtalen før du prøver igjen");
          }
          for (const requestId of [...pendingThreadReplies.keys()]) {
            failPendingThreadReply(requestId, "sambandet vart brote; kontroller tråden før du prøver igjen");
          }
          if (connectionSupervisor.state.heartbeatTimer !== null) {
            window.clearInterval(connectionSupervisor.state.heartbeatTimer);
            connectionSupervisor.state.heartbeatTimer = null;
          }
          if (connectionSupervisor.state.stableConnectionTimer !== null) {
            window.clearTimeout(connectionSupervisor.state.stableConnectionTimer);
            connectionSupervisor.state.stableConnectionTimer = null;
          }
          if (event.code === 1008) {
            recoverAuthentication().catch(() => scheduleReconnect(event.code, event.reason));
            return;
          }
          scheduleReconnect(event.code, event.reason);
        });

        nextSocket.addEventListener("error", () => {
          if (previousSocket && connectionSupervisor.state.socket === previousSocket) return;
          if (connectionSupervisor.state.socket === nextSocket) {
            reportClientEvent("websocket_error");
            setConnected(false, "Mista sambandet");
          }
        });
      }

      function finishSocketHandoff(nextSocket) {
        if (connectionSupervisor.state.socketHandoff?.nextSocket !== nextSocket || connectionSupervisor.state.socket !== nextSocket) return;
        const handoff = connectionSupervisor.state.socketHandoff;
        const previousSocket = handoff.previousSocket;
        if (handoff.timeoutId !== null) window.clearTimeout(handoff.timeoutId);
        connectionSupervisor.state.socketHandoff = null;
        if (previousSocket.readyState === WebSocket.OPEN) {
          previousSocket.close(4000, "session refreshed");
        }
      }

      function scheduleReconnect(closeCode = 1006, closeReason = "") {
        connectionSupervisor.state.reconnectAttempt += 1;
        const delay = Math.min(
          15_000,
          500 * (2 ** Math.min(connectionSupervisor.state.reconnectAttempt - 1, 5))
        );
        const detail = closeReason ? `kode ${closeCode}: ${closeReason}` : `kode ${closeCode}`;
        setConnected(false, `Fråkopla (${detail}) – prøver igjen om ${Math.ceil(delay / 1000)} sekund`);
        connectionSupervisor.state.reconnectTimer = window.setTimeout(() => {
          connectionSupervisor.state.reconnectTimer = null;
          recoverConnection().catch(() => scheduleReconnect(closeCode, closeReason));
        }, delay);
      }

      function sendCommand(type, payload) {
        const currentSocket = connectionSupervisor.state.socket;
        if (!currentSocket || currentSocket.readyState !== WebSocket.OPEN) return null;
        requestNumber += 1;
        const command = {
          protocol: "sproyt.chat.v1",
          request_id: `${browserSessionId}-${requestNumber}`,
          type
        };
        if (payload !== undefined) {
          command.payload = payload;
        }
        currentSocket.send(JSON.stringify(command));
        pendingCommands.set(command.request_id, type);
        if (type === "list_my_channels") latestChannelListRequestId = command.request_id;
        if (type === "list_my_circles") latestCircleListRequestId = command.request_id;
        return command.request_id;
      }

      function finishPendingMessage(requestId, message) {
        const pending = requestId ? pendingMessages.get(requestId) : undefined;
        if (!pending) return;
        if (message?.channel_id !== pending.channelId || message?.body !== pending.body) {
          console.warn("Sendekvitteringa samsvarar ikkje med kommandoen", {
            requestId,
            requestedChannelId: pending.channelId,
            acceptedChannelId: message?.channel_id,
            acceptedMessageId: message?.id
          });
          failPendingMessage(requestId, "tenaren svarte med ei eldre meldingskvittering; utkastet er bevart");
          return;
        }
        pendingMessages.delete(requestId);
        bodyInput.readOnly = false;
        pendingMedia = pendingMedia.filter((media) => !pending.mediaIds.includes(media.id));
        renderMediaPreviews();
        if (message?.channel_id === activeChannelId) {
          setUploadStatus("");
          bodyInput.focus();
        }
        syncComposerState();
        setConnected(connectionSupervisor.state.socket?.readyState === WebSocket.OPEN, "Tilkopla");
      }

      function pendingMessageToReveal(message, requestId = null) {
        if (!message || message.sender_id !== currentParticipantId) return null;
        const requested = requestId ? pendingMessages.get(requestId) : null;
        if (requested?.channelId === message.channel_id && requested.body === message.body) return requested;
        return [...pendingMessages.values()].find((pending) =>
          pending.channelId === message.channel_id && pending.body === message.body
        ) || null;
      }

      function failPendingMessage(requestId, message) {
        const pending = requestId ? pendingMessages.get(requestId) : undefined;
        if (!pending) return;
        pendingMessages.delete(requestId);
        bodyInput.readOnly = false;
        if (bodyInput.value.trim().length === 0) bodyInput.value = pending.draft;
        persistActiveDraft();
        syncComposerState();
        setConnected(connectionSupervisor.state.socket?.readyState === WebSocket.OPEN, `Meldinga vart ikkje sendt: ${message}`);
        bodyInput.focus();
      }

      function finishPendingThreadReply(requestId, message) {
        const pending = requestId ? pendingThreadReplies.get(requestId) : undefined;
        if (!pending) return false;
        pendingThreadReplies.delete(requestId);
        const state = threadComposerState(pending.rootId);
        if (message?.parent_message_id !== pending.rootId || message?.channel_id !== pending.channelId || message?.body !== pending.body) {
          if (activeThreadRootId === pending.rootId && threadBody.value.trim().length === 0) {
            threadBody.value = pending.draft;
          }
          if (state) state.draft = pending.draft;
          if (activeThreadRootId === pending.rootId) threadBody.readOnly = false;
          persistThreadDraft(pending.rootId, pending.channelId);
          setConnected(connectionSupervisor.state.socket?.readyState === WebSocket.OPEN, "Tråden fekk ei ugyldig sendekvittering; svaret er bevart");
          syncThreadComposer();
          return true;
        }
        if (state) state.media = state.media.filter((media) => !pending.mediaIds.includes(media.id));
        if (state) state.draft = "";
        clearThreadDraft(pending.rootId, pending.channelId);
        if (activeThreadRootId === pending.rootId) { threadBody.readOnly = false; renderThreadMediaPreviews(); }
        return true;
      }

      function failPendingThreadReply(requestId, message) {
        const pending = requestId ? pendingThreadReplies.get(requestId) : undefined;
        if (!pending) return false;
        pendingThreadReplies.delete(requestId);
        const state = threadComposerState(pending.rootId);
        if (activeThreadRootId === pending.rootId && threadBody.value.trim().length === 0) {
          threadBody.value = pending.draft;
        }
        if (state) state.draft = pending.draft;
        if (activeThreadRootId === pending.rootId) threadBody.readOnly = false;
        persistThreadDraft(pending.rootId, pending.channelId);
        setConnected(connectionSupervisor.state.socket?.readyState === WebSocket.OPEN, `Trådsvaret vart ikkje sendt: ${message}`);
        syncThreadComposer();
        return true;
      }

      function setConnected(connected, status) {
        applicationStore.updateConnection({ connected, status });
        setConnectionStatus(status);
        const writableChannel = connected
          && activeChannelId !== null
          && connectionSupervisor.state.subscribedChannelId === activeChannelId;
        bodyInput.disabled = !writableChannel;
        sendButton.disabled = !writableChannel || pendingMessages.size > 0;
        attachMediaButton.disabled = !writableChannel || pendingMessages.size > 0;
        messageEmojiPicker.setAttribute("aria-disabled", String(!writableChannel || pendingMessages.size > 0));
        syncThreadComposer();
        circleButtons.forEach((button) => { button.disabled = !connected; });
        exportButton.disabled = !connected;
        processButtons.forEach((button) => { button.disabled = !connected; });
        updateOnboardingButtons();
      }

      function setConnectionStatus(status) {
        applicationStore.updateConnection({ status });
        const connection = applicationStore.snapshot.connection;
        statusEl.textContent = connection.status;
        const routine = connection.status === "Tilkopla";
        const reconnecting = /^(Fornyar økta|Gjenopprettar samtalen|Koplar til)/.test(connection.status);
        statusEl.dataset.routine = String(routine);
        connectionStatusDot.dataset.routine = String(routine);
        connectionStatusDot.dataset.reconnecting = String(reconnecting);
        connectionStatusToggle.setAttribute("aria-label", `Sambandsstatus: ${connection.status}`);
        connectionStatusToggle.title = connection.status;
      }

      function updateOnboardingButtons() {
        const connected = connectionSupervisor.state.socket?.readyState === WebSocket.OPEN;
        createCircleButton.disabled = !connected || circleName.value.trim().length < 2;
        createCircleInvitationButton.disabled = !connected || !circleSelect.value;
        acceptInvitationButton.disabled = !connected || !invitationValueToToken(invitationToken.value);
        const selectedCircle = knownCircles.get(circleSelect.value);
        deleteCircleButton.hidden = selectedCircle?.role !== "owner";
        deleteCircleButton.disabled = !connected || selectedCircle?.role !== "owner";
        const memberChannel = knownChannels.find((channel) => channel.id === channelDetailsDialog.dataset.channelId);
        const canManageMember = connected && channelMember.value && memberChannel && ["owner", "moderator"].includes(memberChannel.role);
        addChannelMember.disabled = !canManageMember;
        inviteChannelMember.disabled = !canManageMember || !memberChannel.circle_id;
      }

      async function processApi(path, method = "GET", body = undefined) {
        const response = await fetch(path, {
          method,
          credentials: "same-origin",
          headers: body === undefined ? {} : { "content-type": "application/json" },
          body: body === undefined ? undefined : JSON.stringify(body)
        });
        const text = await response.text();
        if (!response.ok) throw new Error(text || `HTTP ${response.status}`);
        return text ? JSON.parse(text) : null;
      }

      async function setHeartFeature(enabled) {
        if (!circleSelect.value) {
          pushSystem("Vel ein vennekrets før event-planlegging blir slått på.");
          return;
        }
        try {
          await processApi(`/api/v1/circles/${circleSelect.value}/features/heart-event-planning`, "POST", { enabled });
          pushSystem(enabled ? "Event-planlegging er slått på for kretsen." : "Event-planlegging er slått av.");
        } catch (error) {
          pushSystem(`Kunne ikkje endre event-planlegging: ${error.message}`);
        }
      }

      async function startEventPlanning() {
        if (!activeChannelId || !circleSelect.value) {
          pushSystem("Vel ein kretskanal før du startar planlegging.");
          return;
        }
        try {
          const result = await processApi("/api/v1/processes", "POST", {
            channel_id: activeChannelId,
            request_id: crypto.randomUUID(),
            namespace: "sproyt",
            definition_name: "event-planning",
            definition_version: "1",
            metadata: { title: processTitle.value.trim() || "Event-planlegging" }
          });
          processId.value = result.process_link_id;
          await refreshProcess();
        } catch (error) {
          pushSystem(`Kunne ikkje starte planlegging: ${error.message}`);
        }
      }

      async function refreshProcess() {
        const id = processId.value.trim();
        if (!id) return;
        try {
          renderProcess(await processApi(`/api/v1/processes/${id}`));
        } catch (error) {
          pushSystem(`Kunne ikkje hente prosess: ${error.message}`);
        }
      }

      async function inspectProcess() {
        const id = processId.value.trim();
        if (!id) return;
        try {
          await processApi(`/api/v1/processes/${id}/inspect`, "POST", { request_id: crypto.randomUUID() });
          pushSystem("Heart-status er lagd i den varige køen. Oppdater status om litt.");
        } catch (error) {
          pushSystem(`Kunne ikkje hente Heart-status: ${error.message}`);
        }
      }

      async function answerProcess(answer) {
        const id = processId.value.trim();
        if (!id) return;
        try {
          await processApi(`/api/v1/processes/${id}/messages`, "POST", {
            request_id: crypto.randomUUID(), payload: { answer }
          });
          pushSystem(`Svaret «${answer}» er lagd i den varige køen.`);
        } catch (error) {
          pushSystem(`Kunne ikkje svare på prosessen: ${error.message}`);
        }
      }

      function renderProcess(view) {
        processView.replaceChildren();
        processView.hidden = false;
        const heading = document.createElement("strong");
        heading.textContent = `${view.process.definition_name}: ${view.process.status}`;
        processView.append(heading);
        for (const event of view.events) {
          const article = document.createElement("article");
          article.className = "process-event";
          const meta = document.createElement("span");
          meta.className = "meta";
          meta.textContent = `${event.event_type} · ${event.actor_id}`;
          const payload = document.createElement("pre");
          payload.textContent = JSON.stringify(event.payload, null, 2);
          article.append(meta, payload);
          processView.append(article);
        }
      }

      function setRenderMode(mode) {
        renderMode = mode;
        const showsSource = mode === "raw";
        viewModeToggle.setAttribute("aria-checked", String(showsSource));
        const label = showsSource ? "Vis normalvising" : "Vis kjeldekode";
        viewModeToggle.setAttribute("aria-label", label);
        viewModeToggle.title = label;
        renderTimeline();
      }

      function renderKnownUsers() {
        const selectedUserId = directUser.value;
        directUser.replaceChildren(new Option("Vel brukar", ""));
        knownUsers.filter((user) => user.id !== currentParticipantId).forEach((user) => {
          const handle = mentionHandle(user);
          const status = [user.status_emoji, user.status_text].filter(Boolean).join(" ");
          const label = `${user.display_name} (@${handle})${status ? ` · ${status}` : ""}`;
          directUser.add(new Option(label, user.id));
        });
        if ([...directUser.options].some((option) => option.value === selectedUserId)) {
          directUser.value = selectedUserId;
        }
        refreshChannelMemberOptions(channelDetailsDialog.dataset.channelId);
        const own = knownUsers.find((user) => user.id === currentParticipantId);
        if (own) {
          if (!statusDraft.dirty) {
            statusDraft.emoji = own.status_emoji || "";
            statusDraft.text = own.status_text || "";
            statusEmoji.value = statusDraft.emoji;
            statusText.value = statusDraft.text;
          }
          document.querySelectorAll("#status-emoji-options [data-emoji]").forEach((button) => {
            button.setAttribute("aria-pressed", String(button.dataset.emoji === statusEmoji.value));
          });
          const statusLabel = own.status_text || own.status_emoji
            ? `${own.status_emoji || ""} ${own.status_text || ""}`.trim()
            : "Set status";
          currentStatusIcon.textContent = own.status_emoji || "🙂";
          currentStatusLabel.textContent = statusLabel;
          currentStatus.setAttribute("aria-label", statusLabel);
          currentStatus.title = statusLabel;
          currentStatus.dataset.tooltip = statusLabel;
        }
        openDirect.disabled = !directUser.value;
        if (directMessageDialog.open) {
          directMessageStatus.textContent = directUser.options.length > 1
            ? ""
            : "Ingen andre brukarar er registrerte enno.";
        }
      }

      function openDirectMessageDialog() {
        directUser.value = "";
        openDirect.disabled = true;
        directMessageStatus.textContent = "Hentar fersk personliste …";
        directMessageDialog.showModal();
        directUser.focus();
        if (!sendCommand("list_users")) {
          directMessageStatus.textContent = "Sprøyt er ikkje tilkopla. Vent litt og prøv igjen.";
        }
      }

      function activeProfile(userId) {
        return knownUsers.find((user) => user.id === userId);
      }

      function directChannelLabel(channel) {
        return activeProfile(channel?.direct_user_id)?.display_name || channel?.name || "Direktesamtale";
      }

      function profileStatus(profile) {
        if (!profile || (!profile.status_emoji && !profile.status_text)) return null;
        return {
          symbol: profile.status_emoji || "●",
          text: profile.status_text || "",
          label: [profile.status_emoji, profile.status_text].filter(Boolean).join(" ")
        };
      }

      function appendProfileStatus(target, userId) {
        const status = profileStatus(activeProfile(userId));
        if (!status) return;
        const indicator = document.createElement("span");
        indicator.className = "profile-status";
        indicator.textContent = status.symbol;
        indicator.title = status.label;
        indicator.setAttribute("aria-label", `Status: ${status.label}`);
        target.append(indicator);
      }

      function refreshVisibleProfileStatuses(userId = null) {
        document.querySelectorAll("[data-profile-user-id]").forEach((target) => {
          if (userId && target.dataset.profileUserId !== userId) return;
          target.querySelector(".profile-status")?.remove();
          appendProfileStatus(target, target.dataset.profileUserId);
        });
      }

      function renderConversationIdentity() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        conversationCircle.hidden = true;
        conversationCircle.textContent = "";
        conversationContext.hidden = true;
        conversationContext.replaceChildren();
        conversationPeerStatus.hidden = true;
        conversationPeerStatus.replaceChildren();
        channelPeopleButton.disabled = !channel;
        const channelUsers = channel ? knownChannelUsers.get(channel.id) : null;
        channelPeopleButton.textContent = channelUsers ? `👥 ${channelUsers.length}` : "👥";
        channelPeopleButton.setAttribute("aria-label", channelUsers
          ? `Vis dei ${channelUsers.length} menneska i kanalen`
          : "Vis menneska i kanalen");
        if (channel?.description) {
          renderMarkdown(channel.description, conversationContext);
          conversationContext.hidden = false;
        }
        if (!channel) return;
        conversationTitle.textContent = channel.name;
        conversationCircle.textContent = channel.circle_id
          ? (knownCircles.get(channel.circle_id)?.name || "Vennekrets")
          : (channel.direct_user_id ? "Direktemelding" : "Felles");
        conversationCircle.hidden = false;
        const peer = channel.direct_user_id ? activeProfile(channel.direct_user_id) : null;
        const status = profileStatus(peer);
        if (!peer || !status) return;
        conversationPeerStatus.hidden = false;
        const symbol = document.createElement("span");
        symbol.textContent = status.symbol;
        symbol.title = status.label;
        symbol.setAttribute("aria-label", `Status: ${status.label}`);
        conversationPeerStatus.append(symbol);
        if (status.text) conversationPeerStatus.append(document.createTextNode(` ${status.text}`));
      }

      function renderChannelMembers(channelId) {
        const users = knownChannelUsers.get(channelId) || [];
        const query = channelMemberSearch.value
          .normalize("NFKD")
          .replace(/[\u0300-\u036f]/g, "")
          .toLocaleLowerCase("nb-NO")
          .trim();
        const visibleUsers = query
          ? users.filter((profile) => profile.display_name
            .normalize("NFKD")
            .replace(/[\u0300-\u036f]/g, "")
            .toLocaleLowerCase("nb-NO")
            .includes(query))
          : users;
        if (channelId === activeChannelId) {
          channelPeopleButton.textContent = `👥 ${users.length}`;
          channelPeopleButton.setAttribute("aria-label", `Vis dei ${users.length} menneska i kanalen`);
        }
        channelMemberSearch.disabled = false;
        channelMemberCount.textContent = query
          ? `Viser ${visibleUsers.length} av ${users.length}`
          : `${users.length} menneske`;
        channelMemberList.replaceChildren();
        if (users.length === 0) {
          const empty = document.createElement("li");
          empty.textContent = "Ingen menneske funne.";
          channelMemberList.append(empty);
          return;
        }
        if (visibleUsers.length === 0) {
          const empty = document.createElement("li");
          empty.textContent = "Ingen namn passar søket.";
          channelMemberList.append(empty);
          return;
        }
        visibleUsers.forEach((profile) => {
          const item = document.createElement("li");
          item.dataset.profileUserId = profile.id;
          const name = document.createElement("span");
          name.textContent = profile.display_name;
          item.append(name);
          appendProfileStatus(item, profile.id);
          channelMemberList.append(item);
        });
        refreshChannelMemberOptions(channelId);
      }

      function refreshChannelMemberOptions(channelId) {
        const channel = knownChannels.find((item) => item.id === channelId);
        const memberIds = new Set((knownChannelUsers.get(channelId) || []).map((user) => user.id));
        const eligibleUsers = channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers;
        channelMember.replaceChildren(new Option("Vel brukar", ""));
        eligibleUsers
          .filter((user) => user.id !== currentParticipantId && !memberIds.has(user.id))
          .forEach((user) => {
            const status = [user.status_emoji, user.status_text].filter(Boolean).join(" ");
            channelMember.add(new Option(`${user.display_name}${status ? ` · ${status}` : ""}`, user.id));
          });
        updateOnboardingButtons();
      }

      function showChannelMemberLoadError(channelId, message) {
        channelMemberSearch.disabled = false;
        channelMemberCount.textContent = "";
        channelMemberList.replaceChildren();
        const item = document.createElement("li");
        const text = document.createElement("span");
        text.textContent = message;
        const retry = document.createElement("button");
        retry.type = "button";
        retry.textContent = "Prøv igjen";
        retry.addEventListener("click", () => requestChannelMembers(channelId));
        item.append(text, retry);
        channelMemberList.append(item);
      }

      function requestChannelMembers(channelId) {
        channelMemberSearch.disabled = true;
        channelMemberCount.textContent = "";
        channelMemberList.replaceChildren(Object.assign(document.createElement("li"), { textContent: "Lastar …" }));
        if (!sendCommand("list_channel_users", { channel_id: channelId })) {
          showChannelMemberLoadError(channelId, "Ikkje tilkopla enno. Vent litt og prøv igjen.");
        }
      }

      function openChannelDetails(editDescription = false) {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        if (!channel) return;
        channelDetailsDialog.dataset.channelId = channel.id;
        channelMemberSearch.value = "";
        channelMemberAdd.hidden = !["owner", "moderator"].includes(channel.role);
        channelMemberStatus.textContent = "";
        refreshChannelMemberOptions(channel.id);
        if (channel.circle_id) sendCommand("list_circle_users", { circle_id: channel.circle_id });
        channelDescriptionForm.hidden = channel.role !== "owner";
        channelDescriptionInput.value = channel.description || "";
        channelDescriptionStatus.textContent = "";
        channelDetailsDialog.showModal();
        requestChannelMembers(channel.id);
        if (editDescription && channel.role === "owner") channelDescriptionInput.focus();
      }

      function renderServerEvent(event) {
        if (event.protocol !== "sproyt.chat.v1") {
          pushSystem("Serveren svarte med ein ukjend protokoll.");
          return;
        }
        const payload = event.payload || {};
        const requestedCommand = event.request_id ? pendingCommands.get(event.request_id) : undefined;
        const pendingInvitation = event.request_id ? pendingInvitationResponses.get(event.request_id) : undefined;
        const inspectedInvitationToken = event.request_id ? pendingInvitationInspections.get(event.request_id) : undefined;
        const invitationRecipient = event.request_id ? pendingChannelInvitationRecipients.get(event.request_id) : undefined;
        const directInvitationMessage = event.request_id ? pendingDirectInvitationMessages.get(event.request_id) : undefined;
        if (event.request_id) pendingCommands.delete(event.request_id);
        if (event.request_id) pendingInvitationResponses.delete(event.request_id);
        if (event.request_id) pendingInvitationInspections.delete(event.request_id);
        if (event.request_id) pendingChannelInvitationRecipients.delete(event.request_id);
        if (event.request_id) pendingDirectInvitationMessages.delete(event.request_id);

        if (event.type === "hello") {
          currentParticipantId = payload.participant_id;
          return;
        }

        if (event.type === "users_listed") {
          knownUsers = payload.users;
          renderKnownUsers();
          if (knownChannels.length > 0) renderChannels();
          renderConversationIdentity();
          refreshVisibleProfileStatuses();
          updateMentionSuggestions();
          return;
        }

        if (event.type === "circle_users_listed") {
          knownCircleUsers.set(payload.circle_id, payload.users);
          const memberChannel = knownChannels.find((channel) => channel.id === channelDetailsDialog.dataset.channelId);
          if (channelDetailsDialog.open && memberChannel?.circle_id === payload.circle_id) {
            refreshChannelMemberOptions(memberChannel.id);
          }
          updateMentionSuggestions();
          return;
        }

        if (event.type === "channel_users_listed") {
          knownChannelUsers.set(payload.channel_id, payload.users);
          if (channelDetailsDialog.open && channelDetailsDialog.dataset.channelId === payload.channel_id) {
            renderChannelMembers(payload.channel_id);
          }
          return;
        }

        if (event.type === "channel_description_updated") {
          const channel = knownChannels.find((item) => item.id === payload.channel_id);
          if (channel) channel.description = payload.description;
          channelDescriptionStatus.textContent = "Omtalen er lagra.";
          renderConversationIdentity();
          return;
        }

        if (event.type === "status_updated") {
          knownUsers = [payload.profile, ...knownUsers.filter((user) => user.id !== payload.profile.id)];
          for (const [circleId, users] of knownCircleUsers) {
            if (users.some((user) => user.id === payload.profile.id)) {
              knownCircleUsers.set(circleId, [payload.profile, ...users.filter((user) => user.id !== payload.profile.id)]);
            }
          }
          for (const [channelId, users] of knownChannelUsers) {
            if (users.some((user) => user.id === payload.profile.id)) {
              knownChannelUsers.set(channelId, [payload.profile, ...users.filter((user) => user.id !== payload.profile.id)]);
            }
          }
          if (payload.profile.id === currentParticipantId) statusDraft.dirty = false;
          renderKnownUsers();
          renderConversationIdentity();
          refreshVisibleProfileStatuses(payload.profile.id);
          if (payload.profile.id === currentParticipantId) {
            document.querySelector("#status-editor").open = false;
          }
          return;
        }

        if (event.type === "mentions_listed") {
          knownMentions = payload.mentions;
          renderPrimaryNavigation();
          renderMentionInbox();
          return;
        }

        if (event.type === "mention_read") {
          const mention = knownMentions.find((item) => item.message.id === payload.message_id);
          if (mention) mention.read = true;
          renderPrimaryNavigation();
          renderMentionInbox();
          return;
        }

        if (event.type === "tasks_listed") {
          knownTasks = payload.tasks;
          renderPrimaryNavigation();
          renderTaskInbox();
          return;
        }

        if (event.type === "task_created") {
          knownTasks = [payload.task, ...knownTasks.filter((task) => task.id !== payload.task.id)];
          showInbox("tasks");
          return;
        }

        if (event.type === "task_updated") {
          knownTasks = knownTasks.map((task) => task.id === payload.task.id ? payload.task : task);
          renderPrimaryNavigation();
          renderTaskInbox();
          return;
        }

        if (event.type === "circles_listed") {
          if (event.request_id !== latestCircleListRequestId) return;
          latestCircleListRequestId = null;
          knownCircles.clear();
          circleSelect.replaceChildren(new Option("Ingen", ""));
          payload.circles.forEach(([circle, role]) => {
            knownCircles.set(circle.id, { ...circle, role });
            circleSelect.add(new Option(`${circle.name} (${role})`, circle.id));
          });
          const restoredCircle = restoreActiveCircle();
          if (restoredCircle) sendCommand("list_joinable_channels", { circle_id: restoredCircle });
          updateOnboardingButtons();
          renderChannels();
          return;
        }
        if (event.type === "circle_created") {
          knownCircles.set(payload.circle.id, { ...payload.circle, role: "owner" });
          circleSelect.add(new Option(`${payload.circle.name} (owner)`, payload.circle.id));
          circleSelect.value = payload.circle.id;
          setActiveCircle(payload.circle.id);
          pushSystem(`Vennekretsen ${payload.circle.name} er oppretta.`);
          onboardingNotice.textContent = `${payload.circle.name} er klar. No kan du invitere vener.`;
          circleName.value = "";
          updateOnboardingButtons();
          sendCommand("create_channel", {
            slug: scopedCircleChannelSlug(payload.circle.id, "prat"), name: "Prat", kind: "private", circle_id: payload.circle.id
          });
          return;
        }
        if (event.type === "circle_deleted") {
          const deletedCircleId = payload.circle_id;
          forgetCircleChannel(deletedCircleId);
          const activeChannel = knownChannels.find((channel) => channel.id === activeChannelId);
          if (activeChannel?.circle_id === deletedCircleId) {
            activeChannelId = null;
            connectionSupervisor.state.subscribedChannelId = null;
            restoredChannelId = null;
            try { window.localStorage.removeItem(activeConversationKey); } catch (_) {}
          }
          clearActiveCircle(deletedCircleId);
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          pushSystem("Vennekretsen og den tilhøyrande historikken er sletta.");
          return;
        }
        if (event.type === "circle_left") {
          const departedCircleId = payload.circle_id;
          forgetCircleChannel(departedCircleId);
          if (circleChannelDialog.open) circleChannelDialog.close();
          knownCircles.delete(departedCircleId);
          knownChannels = knownChannels.filter((channel) => channel.circle_id !== departedCircleId);
          clearActiveCircle(departedCircleId);
          if (activeChannelId && !knownChannels.some((channel) => channel.id === activeChannelId)) {
            activeChannelId = null;
            connectionSupervisor.state.subscribedChannelId = null;
            restoredChannelId = null;
            try { window.localStorage.removeItem(activeConversationKey); } catch (_) {}
          }
          onboardingNotice.textContent = "Du har forlate vennekretsen.";
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          return;
        }
        if (event.type === "circle_invitation_created") {
          invitationToken.value = `${window.location.origin}/?invite=${encodeURIComponent(payload.invitation.token)}`;
          copyInvitation.hidden = false;
          onboardingNotice.textContent = "Invitasjonslenkja er klar. Kopier og send henne til venen din.";
          updateOnboardingButtons();
          return;
        }
        if (event.type === "invitation_created") {
          if (invitationRecipient) {
            const directRequestId = sendCommand("open_direct_channel", { user_id: invitationRecipient });
            if (directRequestId) {
              pendingDirectInvitationMessages.set(directRequestId, `[[invite:${payload.invitation.token}]]`);
              channelMemberStatus.textContent = "Opnar direktemeldinga …";
            }
            return;
          }
          invitationToken.value = `[[invite:${payload.invitation.token}]]`;
          copyInvitation.hidden = false;
          onboardingNotice.textContent = "Invitasjonsmeldinga er klar. Kopier henne inn i ein samtale.";
          updateOnboardingButtons();
          return;
        }
        if (event.type === "invitation_inspected" || event.type === "invitation_declined") {
          invitationInspectionCache.set(payload.token, { status: "resolved", invitation: payload.invitation });
          updateInvitationCards(payload.token, payload.invitation);
          return;
        }
        if (event.type === "invitation_accepted") {
          invitationInspectionCache.set(payload.token, { status: "resolved", invitation: payload.invitation });
          markInvitationAccepted(payload.token);
          onboardingNotice.textContent = "Du er med i vennekretsen. Samtalane blir lasta inn no.";
          invitationToken.value = "";
          copyInvitation.hidden = true;
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          pendingInvitationChannel = payload.invitation.channel.id;
          return;
        }
        if (event.type === "circle_invitation_accepted") {
          onboardingNotice.textContent = "Du er med i vennekretsen. Samtalane blir lasta inn no.";
          invitationToken.value = "";
          copyInvitation.hidden = true;
          const cleanUrl = new URL(window.location.href);
          cleanUrl.searchParams.delete("invite");
          window.history.replaceState({}, "", cleanUrl);
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "channels_listed") {
          if (event.request_id !== latestChannelListRequestId) return;
          latestChannelListRequestId = null;
          knownChannels = payload.channels;
          renderChannels();
          renderConversationIdentity();
          updateAgentAccessControls();
          const requested = knownChannels.find((channel) => channel.slug === requestedChannelSlug);
          const current = knownChannels.find((channel) => channel.id === activeChannelId);
          const restored = knownChannels.find((channel) => channel.id === restoredChannelId);
          // Reconnects (including silent OIDC refresh) must keep the active
          // conversation. The requested slug is only a startup fallback.
          const invited = knownChannels.find((channel) => channel.id === pendingInvitationChannel);
          const next = invited || current || restored || requested || knownChannels[0];
          if (invited) pendingInvitationChannel = null;
          if (next && next.id !== activeChannelId) selectChannel(next);
          return;
        }

        if (event.type === "channel_created") {
          knownChannels.push({ ...payload.channel, role: "owner", latest_sequence: 0, last_read_sequence: 0 });
          renderChannels();
          selectChannel(payload.channel);
          managedChannelName.value = "";
          if (circleChannelDialog.open) circleChannelDialog.close();
          onboardingNotice.textContent = `Kanalen ${payload.channel.name} er klar.`;
          updateOnboardingButtons();
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "joinable_channels_listed") {
          const channels = payload.channels.map((item) => ({ ...item.channel, description: item.description || "" }));
          if (managedCircleId && channels.every((channel) => channel.circle_id === managedCircleId)) {
            renderManagedJoinableChannels(channels);
          }
          updateOnboardingButtons();
          return;
        }

        if (event.type === "membership_joined") {
          pendingInvitationChannel = payload.membership.channel_id;
          if (circleChannelDialog.open) circleChannelDialog.close();
          sendCommand("list_my_channels");
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "membership_left") {
          if (payload.channel_id === activeChannelId) {
            activeChannelId = null;
            connectionSupervisor.state.subscribedChannelId = null;
            restoredChannelId = null;
            try { window.localStorage.removeItem(activeConversationKey); } catch (_) {}
          }
          onboardingNotice.textContent = "Du har forlate kanalen. Du kan bli med igjen dersom han er open.";
          sendCommand("list_my_channels");
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "channel_member_added") {
          channelMemberStatus.textContent = "Brukaren er lagd til i kanalen.";
          channelMember.value = "";
          if (channelDetailsDialog.open && channelDetailsDialog.dataset.channelId === payload.membership.channel_id) {
            sendCommand("list_channel_users", { channel_id: payload.membership.channel_id });
          }
          updateOnboardingButtons();
          return;
        }

        if (event.type === "direct_channel_opened") {
          let channel = knownChannels.find((item) => item.id === payload.channel.id);
          if (!channel) {
            channel = { ...payload.channel, latest_sequence: 0, last_read_sequence: 0, role: "member" };
            knownChannels.push(channel);
          }
          renderChannels();
          directMessageStatus.textContent = "";
          directMessageDialog.close();
          selectChannel(channel);
          if (directInvitationMessage) {
            sendCommand("send_message", { channel_id: channel.id, body: directInvitationMessage });
            channelDetailsDialog.close();
          }
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "subscription_started") {
          if (payload.channel_id !== activeChannelId) {
            sendCommand("unsubscribe_channel", { channel_id: payload.channel_id });
            return;
          }
          connectionSupervisor.state.subscribedChannelId = payload.channel_id;
          setConnectionStatus("Tilkopla");
          renderConversationIdentity();
          payload.history.forEach(appendTimelineMessage);
          sendCommand("list_thread_summaries", { channel_id: payload.channel_id });
          historyHasMore = payload.history.length === historyPageSize;
          historyLoading = false;
          acknowledgeLatest(payload.channel_id, payload.history);
          bodyInput.disabled = false;
          sendButton.disabled = false;
          attachMediaButton.disabled = false;
          messageEmojiPicker.setAttribute("aria-disabled", "false");
          syncComposerState();
          renderChannels();
          const scrollOffset = reconnectScrollOffset;
          reconnectScrollOffset = null;
          renderTimeline({ forceBottom: scrollOffset === null || scrollOffset < 80 });
          if (pendingThreadToOpen) {
            const rootMessageId = pendingThreadToOpen;
            pendingThreadToOpen = null;
            window.setTimeout(() => openThread(rootMessageId), 0);
          }
          if (scrollOffset !== null && scrollOffset >= 80) restoreConversationScrollOffset(scrollOffset);
          updateAgentAccessControls();
          finishSocketHandoff(connectionSupervisor.state.socket);
          return;
        }

        if (event.type === "subscription_ended") {
          if (payload.channel_id === connectionSupervisor.state.subscribedChannelId) {
            connectionSupervisor.state.subscribedChannelId = null;
            setConnected(connectionSupervisor.state.socket?.readyState === WebSocket.OPEN, "Koplar til samtalen …");
          }
          return;
        }

        if (event.type === "channel_reactions_listed") {
          if (payload.channel_id === activeChannelId) {
            replaceChannelReactions(payload.reactions);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "message_reaction_changed") {
          if (payload.change.channel_id === activeChannelId) {
            applyReactionChange(payload.change);
            if (!patchMessageReactions(payload.change.message_id)) {
              renderTimeline({ preserveScroll: true });
            }
          }
          return;
        }

        if (event.type === "thread_summaries_listed") {
          if (payload.channel_id !== activeChannelId) return;
          threadSummaries.clear();
          for (const summary of payload.summaries) threadSummaries.set(summary.root_message_id, summary);
          renderTimeline({ preserveScroll: true });
          return;
        }

        if (event.type === "thread_loaded") {
          const root = payload.messages.find((message) => message.id === payload.root_message_id);
          const replies = payload.messages.filter((message) => message.parent_message_id === payload.root_message_id);
          if (root) threadRoots.set(payload.root_message_id, root);
          threadReplies.set(payload.root_message_id, replies);
          if (activeThreadRootId === payload.root_message_id) {
            renderThread();
            const latest = replies.at(-1)?.sequence;
            if (latest !== undefined) sendCommand("mark_thread_read", { root_message_id: payload.root_message_id, sequence: latest });
          }
          return;
        }

        if (event.type === "thread_read_updated") {
          threadSummaries.set(payload.summary.root_message_id, payload.summary);
          renderTimeline({ preserveScroll: true });
          return;
        }

        if (event.type === "chat") {
          const chatEvent = payload.event;
          if (chatEvent.type === "message_accepted") {
            updateLatestSequence(chatEvent.message.channel_id, chatEvent.message.sequence);
            if (chatEvent.message.channel_id === activeChannelId) {
              const revealOwnMessage = pendingMessageToReveal(chatEvent.message);
              appendTimelineMessage(chatEvent.message);
              acknowledgeLatest(activeChannelId, [chatEvent.message]);
              renderTimeline({ revealMessageId: revealOwnMessage ? chatEvent.message.id : null });
            } else {
              renderChannels();
            }
          } else if (chatEvent.type === "message_edited") {
            if (chatEvent.message.channel_id === activeChannelId) {
              replaceTimelineMessage(chatEvent.message);
              renderTimeline({ preserveScroll: true });
            }
          } else if (chatEvent.type === "message_deleted") {
            messageReactions.delete(chatEvent.message.id);
            if (chatEvent.message.channel_id === activeChannelId) {
              replaceTimelineMessage(chatEvent.message);
              renderTimeline({ preserveScroll: true });
            }
          } else if (chatEvent.type === "message_reaction_changed") {
            if (chatEvent.change.channel_id === activeChannelId) {
              applyReactionChange(chatEvent.change);
              if (!patchMessageReactions(chatEvent.change.message_id)) {
                renderTimeline({ preserveScroll: true });
              }
            }
          }
          return;
        }

        if (event.type === "message_accepted") {
          updateLatestSequence(payload.message.channel_id, payload.message.sequence);
          if (payload.message.channel_id === activeChannelId) {
            const revealOwnMessage = pendingMessageToReveal(payload.message, event.request_id);
            appendTimelineMessage(payload.message);
            acknowledgeLatest(activeChannelId, [payload.message]);
            renderTimeline({ revealMessageId: revealOwnMessage ? payload.message.id : null });
          } else {
            renderChannels();
          }
          finishPendingMessage(event.request_id, payload.message);
          finishPendingThreadReply(event.request_id, payload.message);
          return;
        }

        if (event.type === "message_edited") {
          if (payload.message.channel_id === activeChannelId) {
            replaceTimelineMessage(payload.message);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "message_deleted") {
          messageReactions.delete(payload.message.id);
          if (payload.message.channel_id === activeChannelId) {
            replaceTimelineMessage(payload.message);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "lagged") {
          pushSystem(`Klienten låg etter og hoppa over ${payload.skipped} event; lastar inn att.`);
          catchUpTargets.set(payload.channel_id, payload.latest_known_sequence);
          sendCommand("load_recent_messages", {
            channel_id: payload.channel_id,
            after: payload.last_seen_sequence,
            limit: 200
          });
          return;
        }

        if (event.type === "messages_loaded") {
          const olderHistory = historyRequestIds.delete(event.request_id);
          if (olderHistory) {
            historyLoading = false;
            if (payload.channel_id !== activeChannelId) return;
            historyHasMore = payload.messages.length === historyPageSize;
            prependTimelineMessages(payload.messages);
            renderTimeline({ preserveScroll: true });
            return;
          }
          payload.messages.forEach(appendTimelineMessage);
          acknowledgeLatest(payload.channel_id, payload.messages);
          renderTimeline();
          const target = catchUpTargets.get(payload.channel_id);
          const last = payload.messages.at(-1);
          if (target !== undefined && last && last.sequence < target) {
            sendCommand("load_recent_messages", {
              channel_id: payload.channel_id,
              after: last.sequence,
              limit: 200
            });
          } else if (target !== undefined) {
            catchUpTargets.delete(payload.channel_id);
          }
          return;
        }

        if (event.type === "read_marker_updated") {
          const channel = knownChannels.find((item) => item.id === payload.membership.channel_id);
          if (channel) channel.last_read_sequence = payload.membership.last_read_sequence;
          renderChannels();
          return;
        }

        if (event.type === "error") {
          const failedHistory = historyRequestIds.delete(event.request_id);
          if (failedHistory) {
            historyLoading = false;
            historyHasMore = false;
            console.error("Kunne ikkje laste eldre meldingar", {
              requestId: event.request_id,
              command: requestedCommand,
              code: payload.code,
              message: payload.message,
              channelId: activeChannelId
            });
            setConnectionStatus("Kunne ikkje laste eldre meldingar. Nyare meldingar er framleis tilgjengelege.");
            return;
          }
          if (requestedCommand === "send_message") {
            const message = payload.message || payload.code || "ukjend feil";
            if (!failPendingThreadReply(event.request_id, message)) {
              failPendingMessage(event.request_id, message);
            }
            pushSystem(payload.message || payload.code);
            return;
          }
          if (requestedCommand === "accept_invitation") {
            const message = payload.code === "not_found"
              ? "Invitasjonen finst ikkje eller er ikkje gyldig lenger. Be venen din lage ei ny lenkje."
              : payload.code === "permission_denied"
                ? "Du må først vere medlem i vennekretsen før du kan bli med i denne kanalen."
                : "Du kunne ikkje bli med med denne invitasjonen. Kontroller lenkja eller be om ei ny.";
            onboardingNotice.textContent = message;
            if (pendingInvitation) showInvitationError(pendingInvitation.token, message);
            return;
          }
          if (requestedCommand === "decline_invitation") {
            const message = "Invitasjonen kunne ikkje avvisast. Prøv igjen.";
            if (pendingInvitation) showInvitationError(pendingInvitation.token, message);
            return;
          }
          if (requestedCommand === "inspect_invitation") {
            const message = payload.code === "not_found"
              ? "Invitasjonen finst ikkje eller er ikkje gyldig lenger."
              : "Invitasjonen kunne ikkje hentast no.";
            if (inspectedInvitationToken) {
              invitationInspectionCache.set(inspectedInvitationToken, {
                status: payload.code === "not_found" ? "missing" : "failed",
                message
              });
              showInvitationError(inspectedInvitationToken, message);
            }
            return;
          }
          if (requestedCommand === "create_circle") {
            onboardingNotice.textContent = "Vennekretsen kunne ikkje opprettast. Prøv eit anna namn.";
            return;
          }
          if (requestedCommand === "create_channel") {
            onboardingNotice.textContent = "Kanalen kunne ikkje opprettast. Prøv eit anna namn eller prøv igjen.";
            if (circleChannelDialog.open) circleMembershipNotice.textContent = "Kanalen kunne ikkje opprettast. Prøv eit anna namn.";
            updateOnboardingButtons();
            return;
          }
          if (requestedCommand === "update_channel_description") {
            channelDescriptionStatus.textContent = payload.code === "permission_denied"
              ? "Berre eigaren kan endre kanalomtalen."
              : "Kanalomtalen kunne ikkje lagrast. Prøv igjen.";
            return;
          }
          if (requestedCommand === "list_channel_users") {
            const channelId = channelDetailsDialog.dataset.channelId;
            showChannelMemberLoadError(channelId, "Medlemslista kunne ikkje lastast.");
            return;
          }
          if (requestedCommand === "add_channel_member") {
            channelMemberStatus.textContent = payload.code === "permission_denied"
              ? "Brukaren må vere medlem av kretsen, og du må ha tilgang til å leggje til medlemmer."
              : "Brukaren kunne ikkje leggjast til. Prøv igjen.";
            return;
          }
          if (requestedCommand === "create_invitation" && invitationRecipient) {
            channelMemberStatus.textContent = "Invitasjonen kunne ikkje lagast. Prøv igjen.";
            return;
          }
          if (requestedCommand === "open_direct_channel") {
            if (directInvitationMessage) {
              channelMemberStatus.textContent = "Direktemeldinga kunne ikkje opnast. Prøv igjen.";
            } else {
              directMessageStatus.textContent = payload.code === "not_found"
                ? "Brukaren finst ikkje lenger. Lukk dialogen og prøv på nytt."
                : payload.code === "conflict"
                  ? "Du kan ikkje starte ei direktesamtale med deg sjølv."
                  : "Samtalen kunne ikkje opnast. Prøv igjen.";
              openDirect.disabled = !directUser.value;
            }
            return;
          }
          if (requestedCommand === "leave_circle") {
            circleMembershipNotice.textContent = "Vennekretsen kunne ikkje forlatast. Eigaren må slette kretsen i administrasjon.";
            return;
          }
          if (requestedCommand === "leave_channel") {
            onboardingNotice.textContent = payload.code === "permission_denied"
              ? "Standardkanalen Prat kan ikkje forlatast."
              : "Kanalen kunne ikkje forlatast. Prøv igjen.";
            return;
          }
          console.error("Sprøyt-kommando feila", {
            requestId: event.request_id,
            command: requestedCommand || "ukjend",
            code: payload.code,
            message: payload.message,
            channelId: activeChannelId
          });
          const passiveCommands = new Set([
            "list_channel_reactions", "list_thread_summaries", "mark_read",
            "list_users", "list_my_channels", "list_my_circles", "list_mentions", "list_tasks"
          ]);
          if (passiveCommands.has(requestedCommand)) {
            setConnectionStatus(`Kunne ikkje oppdatere samtalen (${requestedCommand || "ukjend"}).`);
            return;
          }
          pushSystem(`${requestedCommand || "Kommando"}: ${payload.message || payload.code}`);
        }
      }

      function renderChannels() {
        renderPrimaryNavigation();
        renderBottomNavigation();
      }

      function renderBottomNavigation() {
        bottomChannelList.replaceChildren();
        bottomCircleContent.replaceChildren();
        const activeCircle = knownCircles.get(activeCircleId);
        const activeChannel = knownChannels.find((channel) => channel.id === activeChannelId);
        const sharedChannels = knownChannels.filter((channel) => !channel.circle_id && !channel.direct_user_id);
        const directChannels = knownChannels.filter((channel) => channel.direct_user_id);
        const showingShared = !activeCircle && activeRootScope === "shared";
        const showingDirect = !activeCircle && activeRootScope === "direct";
        const activeChannelInScope = activeChannel && (activeCircle
          ? activeChannel.circle_id === activeCircleId
          : (showingDirect ? Boolean(activeChannel.direct_user_id) : (!activeChannel.circle_id && !activeChannel.direct_user_id)));
        const channelLabel = activeChannelInScope
          ? (activeChannel.direct_user_id ? directChannelLabel(activeChannel) : `# ${activeChannel.name}`)
          : (showingDirect ? "Direktesamtalar" : "# Kanal");
        const circleLabel = activeCircle?.name || (showingDirect ? "Direkte" : "Felles");
        bottomChannelToggle.querySelector(".bottom-navigation-label").textContent = channelLabel;
        bottomChannelToggle.setAttribute("aria-label", `Vel kanal. Aktiv kanal: ${activeChannelInScope ? (activeChannel.direct_user_id ? directChannelLabel(activeChannel) : activeChannel.name) : "ingen"}`);
        bottomCircleToggle.querySelector(".bottom-navigation-label").textContent = `◎ ${circleLabel}`;
        bottomCircleToggle.setAttribute("aria-label", `Vel område. Aktivt område: ${circleLabel}`);

        const sharedUnreadCount = sharedChannels.reduce(
          (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
          0
        );
        const directUnreadCount = directChannels.reduce(
          (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
          0
        );
        updateCircleToolButtons(sharedUnreadCount, directUnreadCount);
        if (knownCircles.size === 0) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = "Ingen vennekretsar enno";
          bottomCircleContent.append(empty);
        } else {
          for (const [circleId, circle] of knownCircles) {
            const channels = knownChannels.filter((channel) => channel.circle_id === circleId);
            const unreadCount = channels.reduce(
              (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
              0
            );
            const button = document.createElement("button");
            button.type = "button";
            button.textContent = circle.name;
            button.setAttribute("aria-current", circleId === activeCircleId ? "page" : "false");
            if (unreadCount > 0) {
              button.classList.add("has-unread");
              const unread = document.createElement("span");
              unread.className = "unread";
              unread.textContent = approximateUnreadCount(unreadCount);
              unread.setAttribute("aria-label", `${unreadCount} uleste meldingar i ${circle.name}`);
              button.append(unread);
            }
            button.addEventListener("click", () => {
              setActiveCircle(circleId);
              circleSelect.value = circleId;
              sendCommand("list_joinable_channels", { circle_id: circleId });
              closeBottomNavigation(bottomCirclePanel, bottomCircleToggle);
              const preferredChannel = preferredCircleChannel(circleId, channels);
              if (preferredChannel) selectChannel(preferredChannel);
              else renderChannels();
            });
            bottomCircleContent.append(button);
          }
        }

        const scopedChannels = activeCircleId
          ? knownChannels.filter((channel) => channel.circle_id === activeCircleId)
          : (showingDirect ? directChannels : sharedChannels);
        const emptyText = showingDirect
          ? "Ingen direktesamtalar enno."
          : (activeCircleId ? "Ingen kanalar i den valde kretsen." : "Ingen felleskanalar enno.");
        appendBottomChannelButtons(scopedChannels, bottomChannelList, emptyText);
        if (activeCircleId) {
          const discover = document.createElement("button");
          discover.type = "button";
          discover.className = "channel-group-action";
          discover.textContent = "+ Finn fleire kanalar";
          discover.addEventListener("click", () => {
            closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
            openChannelManagement(activeCircleId);
          });
          bottomChannelList.append(discover);
          if (activeChannel?.circle_id === activeCircleId && activeChannel.name.trim().toLocaleLowerCase() !== "prat") {
            const leave = document.createElement("button");
            leave.type = "button";
            leave.className = "channel-group-action danger-button";
            leave.textContent = `Forlat # ${activeChannel.name}`;
            leave.addEventListener("click", () => {
              closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
              if (!window.confirm(`Vil du forlate kanalen ${activeChannel.name}? Du kan bli med igjen seinare dersom kanalen er open.`)) return;
              sendCommand("leave_channel", { channel_id: activeChannel.id });
            });
            bottomChannelList.append(leave);
          }
        } else if (showingDirect) {
          const startDirect = document.createElement("button");
          startDirect.type = "button";
          startDirect.className = "channel-group-action";
          startDirect.textContent = "+ Ny samtale …";
          startDirect.addEventListener("click", () => {
            closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
            openDirectMessageDialog();
          });
          bottomChannelList.append(startDirect);
        }
      }

      function activateRootScope(scope) {
        const channels = knownChannels.filter((channel) => scope === "direct"
          ? Boolean(channel.direct_user_id)
          : (!channel.circle_id && !channel.direct_user_id));
        clearActiveCircle();
        activeRootScope = scope;
        circleSelect.value = "";
        closeBottomNavigation(bottomCirclePanel, bottomCircleToggle);
        const current = channels.find((channel) => channel.id === activeChannelId);
        if (current) renderChannels();
        else if (channels[0]) selectChannel(channels[0]);
        else renderChannels();
      }

      function updateCircleToolButtons(sharedUnreadCount, directUnreadCount) {
        circleToolDirect.setAttribute("aria-pressed", String(!activeCircleId && activeRootScope === "direct"));
        circleToolShared.setAttribute("aria-pressed", String(!activeCircleId && activeRootScope === "shared"));
        circleToolDirect.classList.toggle("has-unread", directUnreadCount > 0);
        circleToolDirect.setAttribute("aria-label", directUnreadCount > 0
          ? `Direkte samtalar, ${directUnreadCount} uleste meldingar`
          : "Direkte samtalar");
        circleToolShared.classList.toggle("has-unread", sharedUnreadCount > 0);
        circleToolShared.setAttribute("aria-label", sharedUnreadCount > 0
          ? `Felles, ${sharedUnreadCount} uleste meldingar`
          : "Felles");
      }

      function appendBottomChannelButtons(channels, target, emptyText = "", panel = bottomChannelPanel, toggle = bottomChannelToggle) {
        if (channels.length === 0 && emptyText) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = emptyText;
          target.append(empty);
          return;
        }
        for (const channel of channels) {
          const unreadCount = Math.max(0, channel.latest_sequence - channel.last_read_sequence);
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = channel.direct_user_id ? directChannelLabel(channel) : `# ${channel.name}`;
          button.setAttribute("aria-current", channel.id === activeChannelId ? "page" : "false");
          if (unreadCount > 0) {
            button.classList.add("has-unread");
            const unread = document.createElement("span");
            unread.className = "unread";
            unread.textContent = approximateUnreadCount(unreadCount);
            unread.setAttribute("aria-label", `${unreadCount} uleste meldingar`);
            button.append(unread);
          }
          button.addEventListener("click", () => {
            closeBottomNavigation(panel, toggle);
            selectChannel(channel);
          });
          target.append(button);
        }
      }

      function closeBottomNavigation(panel, toggle) {
        panel.open = false;
        toggle.focus();
      }

      function openChannelManagement(circleId) {
        const circle = knownCircles.get(circleId);
        if (!circle) return;
        managedCircleId = circleId;
        circleSelect.value = circleId;
        setActiveCircle(circleId);
        updateOnboardingButtons();
        circleChannelTitle.textContent = `Kanalar i ${circle.name}`;
        circleJoinableList.innerHTML = '<p class="status">Lastar tilgjengelege kanalar …</p>';
        const owner = circle.role === "owner";
        leaveCircleButton.hidden = owner;
        circleMembershipNotice.textContent = owner
          ? "Eigaren kan ikkje forlate kretsen. Kretsen kan slettast frå administrasjon."
          : "Du mistar tilgang til kanalane i kretsen, men meldingane dine blir ståande.";
        if (!circleChannelDialog.open) circleChannelDialog.showModal();
        sendCommand("list_joinable_channels", { circle_id: circleId });
        window.setTimeout(() => circleChannelClose.focus(), 0);
      }

      function renderManagedJoinableChannels(channels) {
        circleJoinableList.replaceChildren();
        if (channels.length === 0) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = "Ingen fleire opne kanalar akkurat no.";
          circleJoinableList.append(empty);
          return;
        }
        for (const channel of channels) {
          const card = document.createElement("article");
          card.className = "joinable-channel-card";
          const join = document.createElement("button");
          join.type = "button";
          join.textContent = "Bli med";
          join.setAttribute("aria-label", `Bli med i ${channel.name}`);
          const name = document.createElement("strong");
          name.textContent = `# ${channel.name}`;
          const description = document.createElement("div");
          description.className = "joinable-channel-description";
          if (channel.description) renderMarkdown(channel.description, description);
          else description.textContent = "Ingen kanalomtale enno.";
          join.addEventListener("click", () => sendCommand("join_channel", {
            channel: { type: "id", value: channel.id }
          }));
          card.append(name, description, join);
          circleJoinableList.append(card);
        }
      }

      function updateNavigationCount(id, count, label) {
        const badge = document.querySelector(`#${id}`);
        const button = badge.closest("button");
        badge.hidden = count === 0;
        badge.textContent = count === 0 ? "" : approximateUnreadCount(count);
        badge.setAttribute("aria-label", `${count} ${label}`);
        const navigationLabel = button.dataset.navigationLabel;
        const buttonLabel = count === 0 ? navigationLabel : `${navigationLabel}: ${count} ${label}`;
        button.setAttribute("aria-label", buttonLabel);
        button.title = buttonLabel;
        button.dataset.tooltip = buttonLabel;
        button.classList.toggle("has-unread", count > 0);
      }

      function renderPrimaryNavigation() {
        const unreadCount = knownChannels.reduce(
          (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
          0
        );
        updateNavigationCount("unread-count", unreadCount, "uleste meldingar");
        updateNavigationCount("mention-count", knownMentions.filter((mention) => !mention.read).length, "uleste omtalar");
        updateNavigationCount("task-count", knownTasks.filter((task) => task.status !== "done").length, "opne oppgåver");
        for (const kind of ["unread", "mentions", "tasks"]) {
          document.querySelector(`#show-${kind}`).setAttribute("aria-current", activeInboxKind === kind ? "page" : "false");
        }
      }

      function approximateUnreadCount(count) {
        if (count < 25) return String(count);
        if (count < 50) return "25+";
        if (count < 100) return "50+";
        return "100+";
      }

      function showInbox(kind) {
        if (connectionSupervisor.state.subscribedChannelId) {
          sendCommand("unsubscribe_channel", {
            channel_id: connectionSupervisor.state.subscribedChannelId
          });
        }
        connectionSupervisor.state.subscribedChannelId = null;
        activeChannelId = null;
        activeInboxKind = kind;
        setMobileNavigationOpen(false);
        reconnectScrollOffset = null;
        timeline.length = 0;
        threadReplies.clear();
        threadRoots.clear();
        threadSummaries.clear();
        if (threadPanel.open) threadPanel.close();
        messageReactions.clear();
        seenMessageIds.clear();
        historyRequestIds.clear();
        historyHasMore = false;
        historyLoading = false;
        bodyInput.disabled = true;
        sendButton.disabled = true;
        attachMediaButton.disabled = true;
        messageEmojiPicker.setAttribute("aria-disabled", "true");
        syncComposerState();
        messagesEl.replaceChildren();
        renderConversationIdentity();
        conversationCircle.textContent = "For deg";
        conversationCircle.hidden = false;
        if (kind === "unread") {
          conversationTitle.textContent = "Uleste meldingar";
          const unread = knownChannels
            .filter((channel) => channel.latest_sequence > channel.last_read_sequence)
            .sort((left, right) => (right.latest_sequence - right.last_read_sequence) - (left.latest_sequence - left.last_read_sequence));
          if (unread.length === 0) {
            messagesEl.innerHTML = '<div class="empty-state"><h2>Alt er lese</h2><p>Du har ingen uleste meldingar akkurat no.</p></div>';
          } else {
            const inbox = document.createElement("section");
            inbox.className = "unread-inbox";
            const summary = document.createElement("p");
            summary.className = "unread-inbox-summary";
            const total = unread.reduce((count, channel) => count + channel.latest_sequence - channel.last_read_sequence, 0);
            summary.textContent = `${total} uleste meldingar i ${unread.length} ${unread.length === 1 ? "samtale" : "samtalar"}`;
            inbox.append(summary);
            for (const channel of unread) {
              const button = document.createElement("button");
              button.type = "button";
              button.className = "unread-card";
              const identity = document.createElement("span");
              const name = document.createElement("strong");
              name.textContent = channel.direct_user_id ? directChannelLabel(channel) : `# ${channel.name}`;
              const context = document.createElement("small");
              context.textContent = channel.circle_id
                ? (knownCircles.get(channel.circle_id)?.name || "Vennekrets")
                : (channel.direct_user_id ? "Direktemelding" : "Felles");
              const count = document.createElement("span");
              count.className = "unread";
              const unreadCount = channel.latest_sequence - channel.last_read_sequence;
              count.textContent = approximateUnreadCount(unreadCount);
              count.setAttribute("aria-label", `${unreadCount} uleste meldingar`);
              identity.append(name, context);
              button.append(identity, count);
              button.addEventListener("click", () => selectChannel(channel));
              inbox.append(button);
            }
            messagesEl.append(inbox);
          }
        } else if (kind === "mentions") {
          conversationTitle.textContent = "Omtalar";
          messagesEl.innerHTML = '<div class="empty-state"><h2>Lastar omtalar …</h2></div>';
          sendCommand("list_mentions");
        } else {
          conversationTitle.textContent = "Oppgåver";
          messagesEl.innerHTML = '<div class="empty-state"><h2>Lastar oppgåver …</h2></div>';
          sendCommand("list_tasks");
        }
        renderChannels();
      }

      function renderMentionInbox() {
        if (conversationTitle.textContent !== "Omtalar") return;
        messagesEl.replaceChildren();
        if (knownMentions.length === 0) {
          messagesEl.innerHTML = '<div class="empty-state"><h2>Ingen omtalar</h2><p>Når nokon skriv @namnet-ditt, kjem meldinga hit.</p></div>';
          return;
        }
        for (const mention of knownMentions) {
          const card = document.createElement("article");
          card.className = "message";
          const heading = document.createElement("strong");
          heading.textContent = `${mention.message.sender_display_name} i ${mention.channel_name}`;
          const body = document.createElement("p");
          body.textContent = mention.message.body;
          const actions = document.createElement("div");
          actions.className = "onboarding-actions";
          const open = document.createElement("button");
          open.type = "button";
          open.textContent = "Opne samtalen";
          open.addEventListener("click", () => {
            const channel = knownChannels.find((item) => item.id === mention.message.channel_id);
            if (channel) {
              pendingThreadToOpen = mention.message.parent_message_id || null;
              selectChannel(channel);
            }
          });
          actions.append(open);
          if (!mention.read) {
            const read = document.createElement("button");
            read.type = "button";
            read.textContent = "Marker lesen";
            read.addEventListener("click", () => sendCommand("mark_mention_read", { message_id: mention.message.id }));
            actions.append(read);
          }
          const task = document.createElement("button");
          task.type = "button";
          task.textContent = "Lag oppgåve";
          task.addEventListener("click", () => createTaskFromMention(mention, card));
          actions.append(task);
          card.append(heading, body, actions);
          if (!mention.read) card.dataset.unread = "true";
          messagesEl.append(card);
        }
      }

      function createTaskFromMention(mention, card) {
        if (card.querySelector(".task-editor")) return;
        const editor = document.createElement("form");
        editor.className = "task-editor";
        const title = document.createElement("input");
        title.required = true;
        title.maxLength = 240;
        title.setAttribute("aria-label", "Oppgåvetittel");
        title.value = mention.message.body.replace(/@\S+/g, "").trim();
        const process = document.createElement("input");
        process.setAttribute("aria-label", "Heart-prosess-ID");
        process.placeholder = "Heart-prosess-ID (valfritt)";
        const save = document.createElement("button");
        save.type = "submit";
        save.textContent = "Lagre oppgåve";
        editor.append(title, process, save);
        editor.addEventListener("submit", (event) => {
          event.preventDefault();
          if (!title.value.trim()) return;
          sendCommand("create_task", {
            source_message_id: mention.message.id,
            assignee_id: currentParticipantId,
            title: title.value.trim(),
            process_link_id: process.value.trim() || null
          });
        });
        card.append(editor);
        title.focus();
      }

      function renderTaskInbox() {
        if (conversationTitle.textContent !== "Oppgåver") return;
        messagesEl.replaceChildren();
        if (knownTasks.length === 0) {
          messagesEl.innerHTML = '<div class="empty-state"><h2>Ingen oppgåver</h2><p>Du kan gjere ei @omtale om til ei oppgåve.</p></div>';
          return;
        }
        for (const task of knownTasks) {
          const card = document.createElement("article");
          card.className = "message";
          const heading = document.createElement("strong");
          heading.textContent = task.title;
          const details = document.createElement("p");
          details.textContent = `${task.channel_name}${task.process_link_id ? ` · Heart ${task.process_link_id}` : ""}`;
          const toggle = document.createElement("button");
          toggle.type = "button";
          toggle.textContent = task.status === "done" ? "Opne igjen" : "Ferdig";
          toggle.addEventListener("click", () => sendCommand("set_task_done", {
            task_id: task.id, done: task.status !== "done"
          }));
          card.append(heading, details, toggle);
          if (task.status === "done") card.dataset.done = "true";
          messagesEl.append(card);
        }
      }

      function selectChannel(channel) {
        if (!channel) return;
        if (channel.id === activeChannelId && channel.id === connectionSupervisor.state.subscribedChannelId) return;
        persistActiveDraft();
        setMobileNavigationOpen(false);
        activeInboxKind = null;
        const previousChannelId = connectionSupervisor.state.subscribedChannelId;
        if (previousChannelId) sendCommand("unsubscribe_channel", { channel_id: previousChannelId });
        connectionSupervisor.state.subscribedChannelId = null;
        timeline.length = 0;
        threadReplies.clear();
        threadRoots.clear();
        threadSummaries.clear();
        if (threadPanel.open) threadPanel.close();
        seenMessageIds.clear();
        historyRequestIds.clear();
        historyHasMore = false;
        historyLoading = false;
        messagesEl.replaceChildren();
        activeChannelId = channel.id;
        restoreActiveDraft();
        restoredChannelId = channel.id;
        try { window.localStorage.setItem(activeConversationKey, channel.id); } catch (_) {}
        if (channel.circle_id) {
          rememberCircleChannel(channel);
          setActiveCircle(channel.circle_id);
          circleSelect.value = channel.circle_id;
        } else {
          clearActiveCircle();
          activeRootScope = channel.direct_user_id ? "direct" : "shared";
          circleSelect.value = "";
        }
        reconnectScrollOffset = null;
        closeMentionSuggestions();
        if (channel.circle_id) sendCommand("list_circle_users", { circle_id: channel.circle_id });
        sendCommand("list_channel_reactions", { channel_id: channel.id });
        renderMediaPreviews();
        requestedChannelSlug = channel.slug;
        renderConversationIdentity();
        renderChannels();
        updateAgentAccessControls();
        bodyInput.disabled = true;
        sendButton.disabled = true;
        attachMediaButton.disabled = true;
        messageEmojiPicker.setAttribute("aria-disabled", "true");
        setConnectionStatus("Koplar til samtalen …");
        if (!sendCommand("subscribe_channel", { channel_id: channel.id })) {
          setConnectionStatus("Vent på samband – trykk på samtalen for å prøve igjen");
        }
      }

      function updateAgentAccessControls() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        const canDelegate = channel?.role === "owner" || channel?.role === "moderator";
        createAgentAccessButton.disabled = !canDelegate || temporaryAgentId !== null;
        if (!activeChannelId && temporaryAgentId === null) {
          agentAccessNotice.textContent = "Vel ei samtale for å lage tilgang.";
        } else if (!canDelegate && temporaryAgentId === null) {
          agentAccessNotice.textContent = "Berre eigarar og moderatorar kan gi agenttilgang til denne samtalen.";
        } else if (temporaryAgentId === null) {
          agentAccessNotice.textContent = "Klar til å lage kortliva agenttilgang for denne samtalen.";
        }
      }

      async function agentApi(path, body) {
        const response = await fetch(path, {
          method: "POST",
          credentials: "same-origin",
          headers: { "accept": "application/json", "content-type": "application/json" },
          body: body === undefined ? undefined : JSON.stringify(body)
        });
        if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
        if (response.status === 204) return null;
        return response.json();
      }

      async function createTemporaryAgentAccess() {
        if (!activeChannelId || temporaryAgentId !== null) return;
        createAgentAccessButton.disabled = true;
        agentAccessNotice.textContent = "Lagar kortliva agenttilgang …";
        const expiresAt = new Date(Date.now() + 30 * 60_000).toISOString();
        let created = null;
        try {
          created = await agentApi("/api/v1/agents", {
            display_name: "Kortliva MCP-agent",
            provider: "sproyt-owner-ui",
            service_identity: crypto.randomUUID(),
            purpose: `Kortliva MCP-tilgang til kanal ${activeChannelId}`,
            rate_limit_per_minute: 30,
            expires_at: expiresAt
          });
          for (const scope of ["read_history", "send_messages"]) {
            await agentApi(`/api/v1/agents/${created.agent_id}/grants`, {
              circle_id: null,
              channel_id: activeChannelId,
              scope,
              expires_at: expiresAt
            });
          }
          temporaryAgentId = created.agent_id;
          agentCredential.value = created.credential;
          agentCredential.hidden = false;
          copyAgentCredentialButton.hidden = false;
          revokeAgentAccessButton.hidden = false;
          agentAccessNotice.textContent = `Tilgangen ${created.agent_id} er klar i 30 minutt. Kopier credentialen no, og trekk han tilbake når testen er ferdig.`;
        } catch (error) {
          if (created?.agent_id) {
            await agentApi(`/api/v1/agents/${created.agent_id}/revoke`).catch(() => {});
          }
          agentAccessNotice.textContent = `Kunne ikkje lage agenttilgang: ${error.message}`;
          updateAgentAccessControls();
        }
      }

      async function revokeTemporaryAgentAccess() {
        if (!temporaryAgentId) return;
        revokeAgentAccessButton.disabled = true;
        try {
          await agentApi(`/api/v1/agents/${temporaryAgentId}/revoke`);
          temporaryAgentId = null;
          agentCredential.value = "";
          agentCredential.hidden = true;
          copyAgentCredentialButton.hidden = true;
          revokeAgentAccessButton.hidden = true;
          revokeAgentAccessButton.disabled = false;
          updateAgentAccessControls();
          agentAccessNotice.textContent = "Agenttilgangen er trekt tilbake.";
        } catch (error) {
          revokeAgentAccessButton.disabled = false;
          agentAccessNotice.textContent = `Kunne ikkje trekkje tilbake agenttilgangen: ${error.message}`;
        }
      }

      function updateLatestSequence(channelId, sequence) {
        const channel = knownChannels.find((item) => item.id === channelId);
        if (channel) channel.latest_sequence = Math.max(channel.latest_sequence || 0, sequence);
      }

      function acknowledgeLatest(channelId, messages) {
        if (channelId !== activeChannelId || messages.length === 0 || document.visibilityState === "hidden") return;
        const sequence = messages.at(-1).sequence;
        updateLatestSequence(channelId, sequence);
        sendCommand("mark_read", { channel_id: channelId, sequence });
      }

      document.addEventListener("visibilitychange", () => {
        if (document.visibilityState !== "visible") return;
        resumeAfterBackground();
        sendCommand("list_my_channels");
        if (!activeChannelId) return;
        const visibleMessages = timeline
          .filter((item) => item.type === "message" && item.message.channel_id === activeChannelId)
          .map((item) => item.message);
        acknowledgeLatest(activeChannelId, visibleMessages);
      });

      function pushSystem(text) {
        timeline.push({ type: "system", text });
        renderTimeline();
      }

      function loadOlderHistory() {
        if (!activeChannelId || !historyHasMore || historyLoading || connectionSupervisor.state.subscribedChannelId !== activeChannelId) return;
        const oldest = timeline.find((item) => item.type === "message")?.message;
        if (!oldest) return;
        historyLoading = true;
        const requestId = sendCommand("load_recent_messages", {
          channel_id: activeChannelId,
          before: oldest.sequence,
          limit: historyPageSize
        });
        if (requestId) historyRequestIds.add(requestId);
        else historyLoading = false;
      }

      function renderTimeline({ preserveScroll = false, forceBottom = false, revealMessageId = null } = {}) {
        const previousHeight = messagesEl.scrollHeight;
        const previousTop = messagesEl.scrollTop;
        const wasNearBottom = previousHeight - previousTop - messagesEl.clientHeight < 80;
        const interaction = captureMessageInteraction(messagesEl);
        messagesEl.replaceChildren();
        for (const item of timeline) {
          if (item.type === "message") {
            appendMessage(item.message);
          } else {
            appendSystem(item.text);
          }
        }
        renderMermaidDiagrams();
        restoreMessageInteraction(messagesEl, interaction);
        if (preserveScroll) {
          messagesEl.scrollTop = messagesEl.scrollHeight - previousHeight + previousTop;
        } else if (revealMessageId) {
          revealTimelineMessage(revealMessageId);
        } else if (forceBottom) {
          settleConversationAtBottom();
        } else if (wasNearBottom) {
          messagesEl.scrollTop = messagesEl.scrollHeight;
        }
      }

      function revealTimelineMessage(messageId) {
        const reveal = () => {
          const card = [...messagesEl.querySelectorAll("[data-message-id]")]
            .find((candidate) => candidate.dataset.messageId === messageId);
          if (!card) return;
          const cardRect = card.getBoundingClientRect();
          const viewportRect = messagesEl.getBoundingClientRect();
          const delta = cardRect.bottom - viewportRect.bottom + 12;
          if (delta > 0) messagesEl.scrollTop += delta;
        };
        reveal();
        requestAnimationFrame(() => {
          reveal();
          requestAnimationFrame(reveal);
        });
        window.setTimeout(reveal, 150);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", reveal, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", reveal, { once: true });
        });
      }

      function captureMessageInteraction(container) {
        const picker = container.querySelector(".reaction-picker[open]");
        if (!picker) return null;
        const messageId = picker.closest("[data-message-id]")?.dataset.messageId;
        if (!messageId) return null;
        const input = picker.querySelector("input");
        return {
          messageId,
          customReaction: input?.value || "",
          focusCustomReaction: document.activeElement === input,
          focusReactionSummary: document.activeElement === picker.querySelector("summary")
        };
      }

      function restoreMessageInteraction(container, interaction) {
        if (!interaction) return;
        const card = [...container.querySelectorAll("[data-message-id]")]
          .find((candidate) => candidate.dataset.messageId === interaction.messageId);
        const picker = card?.querySelector(".reaction-picker");
        if (!picker) return;
        card.classList.add("reaction-picker-requested");
        picker.open = true;
        const input = picker.querySelector("input");
        if (input) input.value = interaction.customReaction;
        if (input && interaction.focusCustomReaction) input.focus({ preventScroll: true });
        else if (interaction.focusReactionSummary) picker.querySelector("summary")?.focus({ preventScroll: true });
      }

      function settleConversationAtBottom() {
        const scroll = () => { messagesEl.scrollTop = messagesEl.scrollHeight; };
        scroll();
        requestAnimationFrame(() => {
          scroll();
          requestAnimationFrame(scroll);
        });
        window.setTimeout(scroll, 150);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", scroll, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", scroll, { once: true });
        });
      }

      function restoreConversationScrollOffset(offset) {
        const restore = () => {
          messagesEl.scrollTop = Math.max(0, messagesEl.scrollHeight - messagesEl.clientHeight - offset);
        };
        restore();
        requestAnimationFrame(restore);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", restore, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", restore, { once: true });
        });
      }

      function renderMessage(message) {
        appendTimelineMessage(message);
        renderTimeline();
      }

      function appendTimelineMessage(message) {
        if (seenMessageIds.has(message.id)) return;
        seenMessageIds.add(message.id);
        if (message.parent_message_id) {
          const replies = threadReplies.get(message.parent_message_id) || [];
          replies.push(message);
          replies.sort((left, right) => left.sequence - right.sequence);
          threadReplies.set(message.parent_message_id, replies);
          const previous = threadSummaries.get(message.parent_message_id);
          threadSummaries.set(message.parent_message_id, {
            root_message_id: message.parent_message_id,
            reply_count: (previous?.reply_count || 0) + 1,
            unread_count: activeThreadRootId === message.parent_message_id || message.sender_id === currentParticipantId
              ? (previous?.unread_count || 0)
              : (previous?.unread_count || 0) + 1,
            latest_sequence: message.sequence
          });
          if (activeThreadRootId === message.parent_message_id) {
            renderThread({ revealOwn: message.sender_id === currentParticipantId });
            sendCommand("mark_thread_read", { root_message_id: message.parent_message_id, sequence: message.sequence });
          }
          return;
        }
        timeline.push({ type: "message", message });
      }

      function replaceTimelineMessage(message) {
        if (message.parent_message_id) {
          const replies = threadReplies.get(message.parent_message_id) || [];
          const index = replies.findIndex((candidate) => candidate.id === message.id);
          if (index >= 0) replies[index] = message;
          else replies.push(message);
          replies.sort((left, right) => left.sequence - right.sequence);
          threadReplies.set(message.parent_message_id, replies);
          if (activeThreadRootId === message.parent_message_id) renderThread({ revealOwn: message.sender_id === currentParticipantId });
          return;
        }
        if (threadRoots.has(message.id)) {
          threadRoots.set(message.id, message);
          if (activeThreadRootId === message.id) renderThread();
        }
        const item = timeline.find((candidate) => candidate.type === "message" && candidate.message.id === message.id);
        if (item) item.message = message;
        else if (!threadRoots.has(message.id)) appendTimelineMessage(message);
      }

      function prependTimelineMessages(messages) {
        const older = [];
        for (const message of messages) {
          if (seenMessageIds.has(message.id)) continue;
          seenMessageIds.add(message.id);
          if (message.parent_message_id) {
            const replies = threadReplies.get(message.parent_message_id) || [];
            replies.push(message);
            replies.sort((left, right) => left.sequence - right.sequence);
            threadReplies.set(message.parent_message_id, replies);
            continue;
          }
          older.push({ type: "message", message });
        }
        timeline.unshift(...older);
      }

      function openThread(messageId) {
        persistThreadDraft();
        const wasKnown = threadComposerStates.has(messageId);
        activeThreadRootId = messageId;
        const state = threadComposerState(messageId);
        if (!wasKnown && state) state.draft = restoreThreadDraft(messageId, activeChannelId);
        threadBody.value = state?.draft || "";
        threadUploadStatus.textContent = state?.status || "";
        threadUploadStatus.dataset.kind = state?.statusKind || "progress";
        threadBody.readOnly = false;
        if (!threadPanel.open) threadPanel.showModal();
        threadMessages.innerHTML = '<div class="empty-state"><p>Lastar tråden …</p></div>';
        renderThreadMediaPreviews();
        syncThreadComposer();
        sendCommand("load_thread", { root_message_id: messageId });
      }

      function renderThread({ revealOwn = false } = {}) {
        if (!activeThreadRootId) return;
        const previousHeight = threadMessages.scrollHeight;
        const previousTop = threadMessages.scrollTop;
        const distanceFromBottom = previousHeight - previousTop - threadMessages.clientHeight;
        const wasNearBottom = distanceFromBottom <= 80;
        const root = timeline.find((item) => item.type === "message" && item.message.id === activeThreadRootId)?.message
          || threadRoots.get(activeThreadRootId);
        if (!root) return;
        threadForm.hidden = Boolean(root.deleted_at);
        threadMessages.replaceChildren();
        const rootContainer = document.createElement("div");
        rootContainer.className = "thread-root";
        appendMessage(root, rootContainer, false);
        threadMessages.append(rootContainer);
        for (const reply of threadReplies.get(activeThreadRootId) || []) {
          appendMessage(reply, threadMessages, false);
        }
        if (revealOwn || wasNearBottom) settleThreadAtBottom();
        else threadMessages.scrollTop = threadMessages.scrollHeight - previousHeight + previousTop;
      }

      function settleThreadAtBottom() {
        const scroll = () => { threadMessages.scrollTop = threadMessages.scrollHeight; };
        scroll();
        requestAnimationFrame(() => {
          scroll();
          requestAnimationFrame(scroll);
        });
        window.setTimeout(scroll, 150);
        threadMessages.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", scroll, { once: true });
        });
        threadMessages.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", scroll, { once: true });
        });
      }

      function replaceChannelReactions(reactions) {
        messageReactions.clear();
        for (const reaction of reactions) {
          if (!messageReactions.has(reaction.message_id)) messageReactions.set(reaction.message_id, new Map());
          messageReactions.get(reaction.message_id).set(reaction.emoji, {
            count: reaction.count,
            reactedByMe: reaction.reacted_by_me,
            userIds: reaction.user_ids || []
          });
        }
      }

      function applyReactionChange(change) {
        if (!messageReactions.has(change.message_id)) messageReactions.set(change.message_id, new Map());
        const reactions = messageReactions.get(change.message_id);
        const current = reactions.get(change.emoji) || { count: 0, reactedByMe: false, userIds: [] };
        current.count = change.count;
        current.userIds = current.userIds.filter((userId) => userId !== change.user_id);
        if (change.added) current.userIds.push(change.user_id);
        if (change.user_id === currentParticipantId) current.reactedByMe = change.added;
        if (current.count === 0) reactions.delete(change.emoji);
        else reactions.set(change.emoji, current);
        if (reactions.size === 0) messageReactions.delete(change.message_id);
      }

      function reactionButton(messageId, emoji, reaction) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "reaction-badge";
        button.setAttribute("aria-pressed", String(reaction.reactedByMe));
        button.setAttribute("aria-label", `${emoji}: ${reaction.count} reaksjonar`);
        button.textContent = `${emoji} ${reaction.count}`;
        button.addEventListener("click", () => sendCommand("toggle_message_reaction", {
          message_id: messageId, emoji
        }));
        return button;
      }

      function renderMessageReactions(message, onPickerToggle) {
        const bar = document.createElement("div");
        bar.className = "message-reactions";
        const reactions = messageReactions.get(message.id) || new Map();
        const displayedEmojis = [...reactions.keys()].sort((left, right) => {
          const leftIndex = reactionEmojis.indexOf(left);
          const rightIndex = reactionEmojis.indexOf(right);
          return (leftIndex < 0 ? 999 : leftIndex) - (rightIndex < 0 ? 999 : rightIndex) || left.localeCompare(right);
        });
        for (const emoji of displayedEmojis) {
          const reaction = reactions.get(emoji);
          if (reaction?.count > 0) bar.append(reactionButton(message.id, emoji, reaction));
        }
        const picker = document.createElement("details");
        picker.className = "reaction-picker";
        picker.addEventListener("toggle", () => onPickerToggle?.(picker.open));
        const summary = document.createElement("summary");
        summary.setAttribute("aria-label", "Legg til reaksjon");
        summary.textContent = "😊 +";
        const choices = document.createElement("div");
        for (const emoji of reactionEmojis) {
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = emoji;
          button.setAttribute("aria-label", `Reager med ${emoji}`);
          button.addEventListener("click", () => {
            sendCommand("toggle_message_reaction", { message_id: message.id, emoji });
            picker.open = false;
          });
          choices.append(button);
        }
        const custom = document.createElement("div");
        custom.className = "reaction-custom";
        const customInput = document.createElement("input");
        customInput.type = "search";
        customInput.maxLength = 32;
        customInput.setAttribute("list", "reaction-emoji-catalog");
        customInput.setAttribute("aria-label", "Søk eller lim inn Unicode-emoji");
        customInput.placeholder = "Søk eller lim inn emoji";
        const customButton = document.createElement("button");
        customButton.type = "button";
        customButton.textContent = "Bruk";
        const submitCustomReaction = () => {
          const emoji = customInput.value.trim();
          if (!emoji) return;
          sendCommand("toggle_message_reaction", { message_id: message.id, emoji });
          picker.open = false;
        };
        customButton.addEventListener("click", submitCustomReaction);
        customInput.addEventListener("keydown", (event) => {
          if (event.key === "Enter") { event.preventDefault(); submitCustomReaction(); }
        });
        custom.append(customInput, customButton);
        choices.append(custom);
        picker.append(summary, choices);
        bar.append(picker);
        return bar;
      }

      function messageHasReactions(messageId) {
        return [...(messageReactions.get(messageId)?.values() || [])]
          .some((reaction) => reaction.count > 0);
      }

      function syncMessageReactionDetails(menu, messageId) {
        const details = menu.querySelector(".message-reaction-details");
        const list = details.querySelector("ul");
        list.replaceChildren();
        const reactions = messageReactions.get(messageId) || new Map();
        for (const [emoji, reaction] of reactions) {
          if (reaction.count === 0) continue;
          const names = reaction.userIds.map((userId) => userId === currentParticipantId
            ? "Du"
            : (activeProfile(userId)?.display_name || "Ein ven"));
          const item = document.createElement("li");
          item.textContent = `${emoji} ${names.join(", ")}`;
          list.append(item);
        }
        details.hidden = list.childElementCount === 0;
      }

      function placeMessageMenu(card, footer, menu, thread, messageId) {
        syncMessageReactionDetails(menu, messageId);
        if (messageHasReactions(messageId) || thread) {
          menu.classList.add("footer-menu");
          footer.insertBefore(menu, thread || null);
          return;
        }
        menu.classList.remove("footer-menu");
        card.querySelector(".meta")?.append(menu);
      }

      function patchMessageReactions(messageId) {
        const message = timeline.find((item) => item.type === "message" && item.message.id === messageId)?.message
          || threadRoots.get(messageId)
          || [...threadReplies.values()].flat().find((candidate) => candidate.id === messageId);
        if (!message) return false;
        let patched = false;
        for (const container of [messagesEl, threadMessages]) {
          const card = [...container.querySelectorAll("[data-message-id]")]
            .find((candidate) => candidate.dataset.messageId === messageId);
          const reactions = card?.querySelector(".message-reactions");
          if (!card || !reactions) continue;
          const interaction = captureMessageInteraction(container);
          const nextReactions = renderMessageReactions(message, (open) => {
            card.classList.toggle("reaction-picker-requested", open);
          });
          const thread = reactions.querySelector(".thread-link");
          const menu = card.querySelector(".message-menu");
          if (thread) nextReactions.append(thread);
          if (menu) placeMessageMenu(card, nextReactions, menu, thread, messageId);
          reactions.replaceWith(nextReactions);
          restoreMessageInteraction(container, interaction);
          patched = true;
        }
        return patched;
      }

      function appendMessage(message, target = messagesEl, includeThread = true) {
        const wrapper = document.createElement("article");
        wrapper.className = "message";
        wrapper.dataset.messageId = message.id;

        const meta = document.createElement("div");
        meta.className = "meta";
        const metaText = document.createElement("span");
        metaText.className = "message-meta-text";
        const sender = message.sender_id === currentParticipantId
          ? "Du"
          : (message.sender_display_name || "Ein ven");
        const senderLabel = document.createElement("span");
        senderLabel.textContent = sender;
        senderLabel.dataset.profileUserId = message.sender_id;
        appendProfileStatus(senderLabel, message.sender_id);
        metaText.append(senderLabel);
        const sentAt = new Date(message.sent_at);
        if (!Number.isNaN(sentAt.valueOf())) {
          const timestamp = document.createElement("time");
          timestamp.dateTime = sentAt.toISOString();
          timestamp.title = sentAt.toLocaleString([], { dateStyle: "full", timeStyle: "short" });
          timestamp.textContent = ` · ${formatMessageTimestamp(sentAt)}`;
          metaText.append(timestamp);
        }
        if (message.deleted_at) {
          const deleted = document.createElement("small");
          deleted.textContent = " · sletta";
          deleted.title = new Date(message.deleted_at).toLocaleString();
          metaText.append(deleted);
        } else if (message.edited_at) {
          const edited = document.createElement("small");
          edited.textContent = " · redigert";
          edited.title = new Date(message.edited_at).toLocaleString();
          metaText.append(edited);
        }
        meta.append(metaText);

        const body = document.createElement("div");
        if (message.deleted_at) {
          body.className = "rendered message-tombstone";
          body.textContent = "Meldinga er sletta.";
        } else if (renderMode === "raw") {
          const pre = document.createElement("pre");
          pre.className = "raw-body";
          pre.textContent = message.body;
          body.append(pre);
        } else {
          body.className = "rendered";
          renderMessageBody(message.body, body);
        }

        wrapper.append(meta, body);
        let footer = null;
        if (!message.deleted_at) {
          footer = renderMessageReactions(message, (open) => {
            wrapper.classList.toggle("reaction-picker-requested", open);
          });
          wrapper.append(footer);
        }
        const summary = threadSummaries.get(message.id);
        const replyCount = summary?.reply_count || 0;
        let thread = null;
        if (includeThread && !message.parent_message_id && replyCount > 0) {
          if (!footer) {
            footer = document.createElement("div");
            footer.className = "message-reactions";
            wrapper.append(footer);
          }
          thread = document.createElement("button");
          thread.type = "button";
          thread.className = "thread-link";
          thread.textContent = replyCount === 0 ? "🧵" : `🧵 ${replyCount}`;
          if (summary?.unread_count > 0) thread.textContent += ` · ${summary.unread_count}`;
          thread.title = replyCount === 0 ? "Start ein tråd" : `${replyCount} svar${summary?.unread_count > 0 ? `, ${summary.unread_count} uleste` : ""}`;
          thread.setAttribute("aria-label", replyCount === 0 ? "Start ein tråd" : `Opne tråd med ${replyCount} svar`);
          thread.addEventListener("click", () => openThread(message.id));
          footer.append(thread);
        }
        if (!message.deleted_at) {
          const menu = document.createElement("details");
          menu.className = "message-menu";
          const menuSummary = document.createElement("summary");
          menuSummary.textContent = "…";
          menuSummary.setAttribute("aria-label", "Fleire handlingar for meldinga");
          const menuItems = document.createElement("div");
          const reactionDetails = document.createElement("section");
          reactionDetails.className = "message-reaction-details";
          reactionDetails.hidden = true;
          const reactionHeading = document.createElement("strong");
          reactionHeading.textContent = "Reaksjonar";
          const reactionNames = document.createElement("ul");
          reactionDetails.append(reactionHeading, reactionNames);
          menuItems.append(reactionDetails);
          const addReaction = document.createElement("button");
          addReaction.type = "button";
          addReaction.textContent = "Legg til reaksjon";
          addReaction.addEventListener("click", () => {
            menu.open = false;
            const picker = wrapper.querySelector(".reaction-picker");
            if (!picker) return;
            wrapper.classList.add("reaction-picker-requested");
            picker.open = true;
            picker.querySelector("summary")?.focus({ preventScroll: true });
          });
          menuItems.append(addReaction);
          if (includeThread && !message.parent_message_id) {
            const openThreadButton = document.createElement("button");
            openThreadButton.type = "button";
            openThreadButton.textContent = replyCount === 0 ? "Start tråd" : "Opne tråd";
            openThreadButton.addEventListener("click", () => {
              menu.open = false;
              openThread(message.id);
            });
            menuItems.append(openThreadButton);
          }
          if (message.sender_id === currentParticipantId) {
            const edit = document.createElement("button");
            edit.type = "button";
            edit.textContent = "Rediger";
            edit.addEventListener("click", () => {
              menu.open = false;
            const editor = document.createElement("form");
            editor.className = "message-editor";
            const input = document.createElement("textarea");
            const mediaTokenPattern = /\[\[media:[0-9a-f-]{36}\|[^|\]]+\|[^\]]*\]\]/gi;
            const mediaTokens = message.body.match(mediaTokenPattern) || [];
            input.value = message.body.replace(mediaTokenPattern, "").trim();
            input.setAttribute("aria-label", "Rediger melding");
            const controls = document.createElement("div");
            const cancel = document.createElement("button");
            cancel.type = "button";
            cancel.textContent = "Avbryt";
            const save = document.createElement("button");
            save.type = "submit";
            save.textContent = "Lagre";
            controls.append(cancel, save);
            editor.append(input, controls);
            body.hidden = true;
            menu.hidden = true;
            wrapper.insertBefore(editor, wrapper.querySelector(".message-reactions"));
            cancel.addEventListener("click", () => { editor.remove(); body.hidden = false; menu.hidden = false; });
            editor.addEventListener("submit", (event) => {
              event.preventDefault();
              const value = input.value.trim();
              const updatedBody = [value, ...mediaTokens].filter(Boolean).join("\n");
              if (!updatedBody || updatedBody === message.body) { cancel.click(); return; }
              sendCommand("edit_message", { message_id: message.id, body: updatedBody });
              cancel.click();
            });
            input.focus();
          });
            const remove = document.createElement("button");
            remove.type = "button";
            remove.textContent = "Slett";
            remove.addEventListener("click", () => {
              if (window.confirm("Vil du slette meldinga? Ho blir ståande som ei sletta melding i samtalen.")) {
                sendCommand("delete_message", { message_id: message.id });
              }
            });
            menuItems.append(edit, remove);
          }
          menu.append(menuSummary, menuItems);
          placeMessageMenu(wrapper, footer, menu, thread, message.id);
        }
        target.append(wrapper);
      }

      function formatMessageTimestamp(sentAt, now = new Date()) {
        const sameDay = sentAt.getFullYear() === now.getFullYear()
          && sentAt.getMonth() === now.getMonth()
          && sentAt.getDate() === now.getDate();
        if (sameDay) return sentAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        const options = { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" };
        if (sentAt.getFullYear() !== now.getFullYear()) options.year = "numeric";
        return sentAt.toLocaleString([], options);
      }

      function renderSystem(text) {
        pushSystem(text);
      }

      function appendSystem(text) {
        const line = document.createElement("div");
        line.className = "system";
        line.textContent = text;
        messagesEl.append(line);
      }

      sessionSupervisor.start();
      connectionSupervisor.start();
      const invitationFromUrl = new URL(window.location.href).searchParams.get("invite");
      if (invitationFromUrl) {
        invitationToken.value = window.location.href;
        onboardingNotice.textContent = "Du er invitert til ein vennekrets. Trykk «Bli med» for å godta.";
        updateOnboardingButtons();
      }

      let pendingInvitationChannel = null;

      function renderMessageBody(source, target) {
        const token = /\[\[media:([0-9a-f-]{36})\|([^|\]]+)\|([^\]]*)\]\]/gi;
        const attachments = [];
        const invitations = [];
        const withoutInvitations = source.replace(/\[\[invite:([A-Za-z0-9_-]{32,128})\]\]/g, (_, invitationToken) => {
          invitations.push(invitationToken);
          return "";
        });
        const text = withoutInvitations.replace(token, (_, id, contentType, encodedName) => {
          let name = "media";
          try { name = decodeURIComponent(encodedName || "media"); } catch (_) {}
          attachments.push({ id, contentType, name });
          return "";
        }).trim();
        if (text) renderMarkdown(text, target);
        invitations.forEach((invitationToken) => renderInvitationCard(invitationToken, target));
        attachments.forEach((media) => {
          const figure = document.createElement("figure");
          figure.className = "message-media";
          const element = media.contentType.startsWith("video/") ? document.createElement("video") : document.createElement("img");
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const originalUrl = `/api/v1/media/${media.id}${authQuery}`;
          element.src = media.contentType.startsWith("image/")
            ? `/api/v1/media/${media.id}/preview${authQuery}`
            : originalUrl;
          if (element instanceof HTMLVideoElement) { element.controls = true; element.preload = "metadata"; }
          else {
            element.alt = media.name;
            element.loading = "lazy";
            element.tabIndex = 0;
            element.setAttribute("role", "button");
            element.setAttribute("aria-label", `Vis ${media.name} i full storleik`);
            const open = () => openMediaLightbox(originalUrl, media.name);
            element.addEventListener("click", open);
            element.addEventListener("keydown", (event) => {
              if (event.key === "Enter" || event.key === " ") { event.preventDefault(); open(); }
            });
          }
          const caption = document.createElement("figcaption");
          caption.textContent = media.name;
          figure.append(element, caption);
          target.append(figure);
        });
      }

      function renderInvitationCard(token, target) {
        const card = document.createElement("section");
        card.className = "invitation-card";
        card.dataset.invitationToken = token;
        card.innerHTML = "<p>Lastar invitasjonen …</p>";
        target.append(card);
        requestInvitationInspection(token);
      }

      function requestInvitationInspection(token, force = false) {
        const cached = invitationInspectionCache.get(token);
        if (cached?.status === "pending" || cached?.status === "missing") {
          if (cached.message) showInvitationError(token, cached.message);
          return;
        }
        if (!force && cached?.status === "resolved") {
          updateInvitationCards(token, cached.invitation);
          return;
        }
        if (!force && cached?.status === "failed") {
          showInvitationError(token, cached.message);
          return;
        }
        const requestId = sendCommand("inspect_invitation", { token });
        if (!requestId) {
          showInvitationError(token, "Invitasjonen kan ikkje hentast medan sambandet er borte.");
          return;
        }
        invitationInspectionCache.set(token, { status: "pending" });
        pendingInvitationInspections.set(requestId, token);
      }

      function refreshVisibleInvitationCards() {
        if (document.visibilityState === "hidden") return;
        const tokens = new Set(
          [...document.querySelectorAll(".invitation-card")]
            .map((card) => card.dataset.invitationToken)
            .filter(Boolean)
            .slice(0, 20)
        );
        tokens.forEach((token) => requestInvitationInspection(token, true));
      }

      function updateInvitationCards(token, invitation) {
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (card.dataset.invitationToken !== token) return;
          const accepted = invitation.response === "accepted";
          const declined = invitation.response === "declined";
          const authoredByMe = invitation.invited_by === currentParticipantId;
          const targetName = invitation.channel_name ? `kanalen ${invitation.channel_name}` : `vennekretsen ${invitation.circle_name}`;
          card.classList.toggle("declined", declined);
          card.removeAttribute("aria-busy");
          card.replaceChildren();
          const heading = document.createElement("h4");
          heading.textContent = `Invitasjon til ${targetName}`;
          const detail = document.createElement("p");
          if (authoredByMe) {
            const responses = [];
            if (invitation.accepted_count > 0) responses.push(`${invitation.accepted_count} har godteke`);
            if (invitation.declined_count > 0) responses.push(`${invitation.declined_count} har avvist`);
            detail.textContent = responses.length > 0
              ? `Du sende invitasjonen. ${responses.join(", ")}.`
              : "Du sende invitasjonen. Ventar på svar.";
          } else {
            detail.textContent = accepted ? "Du har godteke invitasjonen." : `${invitation.invited_by_name} har invitert deg.`;
          }
          card.append(heading, detail);
          if (!accepted && !authoredByMe) {
            const actions = document.createElement("div"); actions.className = "invitation-actions";
            const accept = document.createElement("button"); accept.type = "button"; accept.textContent = declined ? "Godta likevel" : "Godta";
            accept.addEventListener("click", () => respondToInvitation(token, "accept_invitation", "Godtek invitasjonen …"));
            const decline = document.createElement("button"); decline.type = "button"; decline.textContent = "Avvis"; decline.disabled = declined;
            decline.addEventListener("click", () => respondToInvitation(token, "decline_invitation", "Avviser invitasjonen …"));
            actions.append(accept, decline); card.append(actions);
          }
        });
      }

      function respondToInvitation(token, command, pendingText) {
        const requestId = sendCommand(command, { token });
        if (!requestId) {
          showInvitationError(token, "Vent til sambandet er tilbake, og prøv igjen.");
          return;
        }
        pendingInvitationResponses.set(requestId, { token, command });
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (card.dataset.invitationToken !== token) return;
          card.setAttribute("aria-busy", "true");
          const detail = card.querySelector("p");
          if (detail) detail.textContent = pendingText;
          card.querySelectorAll(".invitation-actions button").forEach((button) => { button.disabled = true; });
        });
      }

      function showInvitationError(token, message) {
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (card.dataset.invitationToken !== token) return;
          card.removeAttribute("aria-busy");
          const detail = card.querySelector("p");
          if (detail) detail.textContent = message;
          detail?.setAttribute("role", "alert");
          card.querySelectorAll(".invitation-actions button").forEach((button) => { button.disabled = false; });
        });
      }

      function markInvitationAccepted(token) {
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (card.dataset.invitationToken !== token) return;
          card.removeAttribute("aria-busy");
          card.classList.remove("declined");
          const detail = card.querySelector("p");
          if (detail) detail.textContent = "Du har godteke invitasjonen.";
          card.querySelector(".invitation-actions")?.remove();
        });
      }

      function openMediaLightbox(url, name) {
        mediaLightboxImage.src = url;
        mediaLightboxImage.alt = name;
        mediaLightboxCaption.textContent = name;
        mediaLightbox.showModal();
      }

      function renderMarkdown(source, target) {
        const lines = source.replace(/\r\n/g, "\n").split("\n");
        let paragraph = [];
        let list = null;
        let inFence = false;
        let fenceLanguage = "";
        let fenceLines = [];

        const flushParagraph = () => {
          if (paragraph.length === 0) {
            return;
          }
          const p = document.createElement("p");
          appendInline(p, paragraph.join(" "));
          target.append(p);
          paragraph = [];
        };

        const flushList = () => {
          if (!list) {
            return;
          }
          target.append(list.element);
          list = null;
        };

        const flushFence = () => {
          const code = fenceLines.join("\n");
          if (fenceLanguage.toLowerCase() === "mermaid") {
            const shell = document.createElement("div");
            shell.className = "mermaid-shell";
            const diagram = document.createElement("div");
            diagram.className = "mermaid";
            diagram.textContent = code;
            shell.append(diagram);
            target.append(shell);
          } else {
            const pre = document.createElement("pre");
            const codeEl = document.createElement("code");
            if (fenceLanguage) {
              codeEl.dataset.language = fenceLanguage;
            }
            codeEl.textContent = code;
            pre.append(codeEl);
            target.append(pre);
          }
          inFence = false;
          fenceLanguage = "";
          fenceLines = [];
        };

        for (const line of lines) {
          const fence = line.match(/^```([A-Za-z0-9_-]+)?\s*$/);
          if (fence) {
            if (inFence) {
              flushFence();
            } else {
              flushParagraph();
              flushList();
              inFence = true;
              fenceLanguage = fence[1] || "";
              fenceLines = [];
            }
            continue;
          }

          if (inFence) {
            fenceLines.push(line);
            continue;
          }

          if (/^\s*$/.test(line)) {
            flushParagraph();
            flushList();
            continue;
          }

          const heading = line.match(/^(#{1,3})\s+(.+)$/);
          if (heading) {
            flushParagraph();
            flushList();
            const level = String(heading[1].length);
            const h = document.createElement(`h${level}`);
            appendInline(h, heading[2]);
            target.append(h);
            continue;
          }

          const quote = line.match(/^>\s?(.+)$/);
          if (quote) {
            flushParagraph();
            flushList();
            const blockquote = document.createElement("blockquote");
            appendInline(blockquote, quote[1]);
            target.append(blockquote);
            continue;
          }

          const unordered = line.match(/^\s*[-*]\s+(.+)$/);
          const ordered = line.match(/^\s*\d+\.\s+(.+)$/);
          if (unordered || ordered) {
            flushParagraph();
            const kind = ordered ? "ol" : "ul";
            if (!list || list.kind !== kind) {
              flushList();
              list = { kind, element: document.createElement(kind) };
            }
            const li = document.createElement("li");
            appendInline(li, (unordered || ordered)[1]);
            list.element.append(li);
            continue;
          }

          flushList();
          paragraph.push(line.trim());
        }

        if (inFence) {
          flushFence();
        }
        flushParagraph();
        flushList();
      }

      function appendInline(parent, text) {
        const parts = text.split(/(`[^`]+`)/g);
        for (const part of parts) {
          if (part.startsWith("`") && part.endsWith("`") && part.length > 1) {
            const code = document.createElement("code");
            code.textContent = part.slice(1, -1);
            parent.append(code);
          } else if (part.length > 0) {
            appendLinkedText(parent, part);
          }
        }
      }

      function appendLinkedText(parent, text) {
        const urlPattern = /https?:\/\/[^\s<>]+/gi;
        let offset = 0;
        for (const match of text.matchAll(urlPattern)) {
          const start = match.index ?? 0;
          if (start > offset) parent.append(document.createTextNode(text.slice(offset, start)));
          let href = match[0];
          let suffix = "";
          while (/[),.!?:;\]}]$/.test(href)) {
            suffix = href.slice(-1) + suffix;
            href = href.slice(0, -1);
          }
          const link = document.createElement("a");
          link.href = href;
          link.target = "_blank";
          link.rel = "noopener noreferrer";
          link.referrerPolicy = "no-referrer";
          link.title = href;
          link.textContent = readableLinkLabel(href);
          parent.append(link);
          if (suffix) parent.append(document.createTextNode(suffix));
          offset = start + match[0].length;
        }
        if (offset < text.length) parent.append(document.createTextNode(text.slice(offset)));
      }

      function readableLinkLabel(href) {
        try {
          const url = new URL(href);
          const host = url.hostname.replace(/^www\./i, "");
          const path = url.pathname === "/" ? "" : url.pathname.replace(/\/$/, "");
          const label = `${host}${path}`;
          return label.length > 72 ? `${label.slice(0, 69)}…` : label;
        } catch (_) {
          return href.length > 72 ? `${href.slice(0, 69)}…` : href;
        }
      }

      async function renderMermaidDiagrams() {
        if (renderMode !== "view") {
          return;
        }
        const diagrams = [...messagesEl.querySelectorAll(".mermaid")];
        if (diagrams.length === 0) return;
        if (mermaidPromise === null) {
          mermaidPromise = import("https://cdn.jsdelivr.net/npm/mermaid@11.16.0/dist/mermaid.esm.min.mjs")
            .then(({ default: mermaid }) => {
              mermaid.initialize({
                startOnLoad: false,
                securityLevel: "strict",
                theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default"
              });
              return mermaid;
            });
        }
        let mermaid;
        try {
          mermaid = await mermaidPromise;
        } catch (_) {
          diagrams.forEach((diagram) => { diagram.textContent = "Diagrammet kunne ikkje lastast."; });
          return;
        }
        for (const diagram of diagrams) {
          if (diagram.dataset.rendered) {
            continue;
          }
          diagram.dataset.rendered = "true";
          try {
            await mermaid.run({ nodes: [diagram] });
          } catch (error) {
            diagram.textContent = `Mermaid-feil: ${error.message || error}`;
          }
        }
      }
    
