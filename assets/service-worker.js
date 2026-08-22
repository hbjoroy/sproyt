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
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
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
  const destination = new URL(event.notification.data?.navigate || "/", self.location.origin).href;
  event.waitUntil((async () => {
    const windows = await clients.matchAll({ type: "window", includeUncontrolled: true });
    const existing = windows.find((client) => client.url.startsWith(self.location.origin));
    if (existing) {
      await existing.navigate(destination);
      return existing.focus();
    }
    return clients.openWindow(destination);
  })());
});
