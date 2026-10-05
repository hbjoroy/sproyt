const CACHE = "sproyt-shell-v1";
const SHELL = [
  "/offline",
  "/manifest.webmanifest",
  "/assets/sproyt-wave-icon-192.png",
  "/assets/sproyt-wave-icon-512.png"
];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (request.method === "POST" && url.pathname === "/share-target") {
    event.respondWith((async () => {
      try {
        const generation = await self.SproytShareInbox.generation();
        await self.SproytShareInbox.capture(await request.formData(), generation);
        const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
        windows.forEach(client => client.postMessage({ type: "share-received" }));
        return Response.redirect(new URL("/share-target", self.location.origin), 303);
      } catch (error) {
        return new Response(`Delinga er ikkje teken imot. ${error instanceof Error ? error.message : "Prøv igjen."}`, {
          status: 503, headers: { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-store" }
        });
      }
    })());
    return;
  }
  if (request.method !== "GET") return;
  if (url.pathname === "/auth/logout") {
    event.respondWith((async () => {
      try { await self.SproytShareInbox.logout(); }
      catch {
        const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
        windows.forEach(client => client.postMessage({ type: "share-cleanup-warning" }));
      }
      // Signing out must not depend on an optional local share store.
      return fetch(request);
    })());
    return;
  }
  if (url.pathname === "/share-target") {
    event.respondWith(fetch(request).catch(() => new Response("Delinga er lagra lokalt. Opne Sprøyt når nettet er tilbake for å velje kanal og sende. Ingenting er sendt enno.", {
      headers: { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-store" }
    })));
    return;
  }
  if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/auth/") || url.pathname === "/ws") return;

  if (request.mode === "navigate") {
    event.respondWith(fetch(request).catch(() => caches.match("/offline")));
    return;
  }
  if (SHELL.includes(url.pathname)) {
    event.respondWith(caches.match(request).then((cached) => cached || fetch(request)));
  }
});

self.addEventListener("push", (event) => {
  if (!event.data) return;
  event.waitUntil((async () => {
    const payload = event.data.json();
    const notification = payload.notification || payload.web_push?.notification;
    if (!notification?.title) return;
    await self.registration.showNotification(notification.title, {
      body: notification.body,
      icon: "/assets/sproyt-wave-icon-192.png",
      badge: "/assets/sproyt-wave-icon-192.png",
      tag: notification.tag,
      data: { navigate: notification.navigate || "/" }
    });
  })());
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const requested = new URL(event.notification.data?.navigate || "/", self.location.origin);
  const destination = requested.origin === self.location.origin ? requested.href : self.location.origin + "/";
  event.waitUntil((async () => {
    const windows = await clients.matchAll({ type: "window", includeUncontrolled: true });
    const existing = windows.find((client) => new URL(client.url).origin === self.location.origin);
    if (existing) {
      try {
        const navigated = await existing.navigate(destination);
        if (navigated) return navigated.focus();
      } catch { /* An old tab may have closed while the push was opened. */ }
    }
    return clients.openWindow(destination);
  })());
});
