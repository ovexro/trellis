// Trellis PWA Service Worker
// Caches the app shell (HTML + manifest) for offline display.
// API requests always go to network.
var CACHE_NAME = 'trellis-v1';
var SHELL_URLS = ['/', '/manifest.json'];

self.addEventListener('install', function(e) {
  e.waitUntil(
    caches.open(CACHE_NAME)
      .then(function(c) { return c.addAll(SHELL_URLS); })
      .then(function() { return self.skipWaiting(); })
  );
});

self.addEventListener('activate', function(e) {
  e.waitUntil(
    caches.keys().then(function(names) {
      return Promise.all(
        names.filter(function(n) { return n !== CACHE_NAME; })
             .map(function(n) { return caches.delete(n); })
      );
    }).then(function() { return self.clients.claim(); })
  );
});

self.addEventListener('fetch', function(e) {
  if (e.request.method !== 'GET') return;
  var url = new URL(e.request.url);
  // API, proxy, and WS calls: always network, never cache
  if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/proxy/') || url.pathname === '/ws') return;
  // Shell assets: network-first with cache fallback
  e.respondWith(
    fetch(e.request).then(function(resp) {
      var clone = resp.clone();
      caches.open(CACHE_NAME).then(function(c) { c.put(e.request, clone); });
      return resp;
    }).catch(function() {
      return caches.match(e.request);
    })
  );
});
