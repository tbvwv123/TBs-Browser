"""0단계 시험용 로컬 서버 (127.0.0.1:8765). 캐시·서비스 워커·로그인 폼 시험 페이지를 낸다."""
import http.server, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
RED = open(os.path.join(HERE, '..', 'icons', 'red.png'), 'rb').read()
COLOR_JS = """
function colorOf(img){const c=document.createElement('canvas');c.width=c.height=1;const x=c.getContext('2d');
x.drawImage(img,0,0);const d=x.getImageData(0,0,1,1).data;return d[0]>d[2]?'red(원본)':'blue(바꿔치기)';}
async function waitImg(img){ if(!img.complete) await new Promise(r=>{img.onload=r;img.onerror=r;}); return img.naturalWidth?colorOf(img):'error'; }
"""
DRM_JS = """
shaka.polyfill.installAll(); const p = new shaka.Player(); p.attach(document.getElementById('v'));
p.configure({drm: {servers: {'com.widevine.alpha': 'https://cwip-shaka-proxy.appspot.com/no_auth'}}});
p.addEventListener('error', e => window.__drm = 'error ' + e.detail.code);
p.load('https://storage.googleapis.com/shaka-demo-assets/angel-one-widevine/dash.mpd').then(() => window.__drm = 'loaded', e => window.__drm = 'error ' + e.code);
"""
PAGES = {
 '/': '<h1>S0 test site</h1>',
 '/cache.html': '<img id=a src="/img-cache.png"><script>'+COLOR_JS+'</script>',
 '/sw.html': '<img id=a src="/img-sw.png"><img id=b src="/img-swcache.png"><script>'+COLOR_JS+
   "navigator.serviceWorker.register('/sw.js').then(()=>navigator.serviceWorker.ready).then(()=>window.__swready=1);</script>",
 '/fresh.html': '<img id=a src="/img-fresh.png"><script>'+COLOR_JS+'</script>',
 '/drm.html': '<video id=v muted autoplay controls width=640></video>'
   '<script src="https://cdnjs.cloudflare.com/ajax/libs/shaka-player/4.3.6/shaka-player.compiled.js"></script>'
   + '<script>' + DRM_JS + '</script>',
 '/login.html': '<form method=post action="/login" id=f><input name=username id=u autocomplete=username>'
   '<input type=password name=password id=p autocomplete=current-password><button id=s>로그인</button></form>',
}
SW = """
self.addEventListener('install', e => { self.skipWaiting(); e.waitUntil(caches.open('s0').then(c => c.add('/img-swcache.png'))); });
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', e => {
  const u = new URL(e.request.url);
  if (u.pathname === '/img-swcache.png') e.respondWith(caches.match(e.request).then(r => r || fetch(e.request)));
  else if (u.pathname === '/img-sw.png') e.respondWith(fetch(e.request));
});
"""
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def send(self, body, ctype, cache='no-store'):
        self.send_response(200); self.send_header('Content-Type', ctype); self.send_header('Cache-Control', cache)
        self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        p = self.path.split('?')[0]
        sys.stdout.write('GET ' + p + '\n'); sys.stdout.flush()
        if p in PAGES: return self.send(('<!doctype html><meta charset=utf-8>' + PAGES[p]).encode(), 'text/html; charset=utf-8')
        if p == '/sw.js': return self.send(SW.encode(), 'text/javascript')
        if p == '/img-cache.png': return self.send(RED, 'image/png', 'max-age=3600')
        if p.startswith('/img-'): return self.send(RED, 'image/png')
        self.send_response(404); self.end_headers()
    def do_POST(self):
        n = int(self.headers.get('Content-Length', 0)); self.rfile.read(n)
        self.send(b'<!doctype html><meta charset=utf-8>logged in', 'text/html; charset=utf-8')
http.server.ThreadingHTTPServer(('127.0.0.1', 8765), H).serve_forever()
