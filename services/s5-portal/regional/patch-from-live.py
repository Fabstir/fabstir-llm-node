# Turns the live portal's server.js into a region-configurable one: every site-specific constant
# becomes an environment variable. No logic is changed.
import re, sys
s = open('/opt/s5-proxy/server.live.js').read()
def sub(pattern, repl, count=1):
    global s
    s, n = re.subn(pattern, repl, s, count=count, flags=re.M)
    if n != count: sys.exit(f'PATCH FAILED: {pattern!r} matched {n}, wanted {count}')
sub(r"^const BASE_SEPOLIA_RPC = '[^']*';", "const BASE_SEPOLIA_RPC = process.env.BASE_SEPOLIA_RPC_URL;\nif (!BASE_SEPOLIA_RPC) { console.error('BASE_SEPOLIA_RPC_URL is required'); process.exit(1); }")
sub(r"^const STORAGE_BASE = '[^']*';", "const STORAGE_BASE = process.env.S5_PROXY_STORAGE_BASE;\nconst STORAGE_HOST = process.env.S5_PROXY_STORAGE_HOST || '';\nif (!STORAGE_BASE) { console.error('S5_PROXY_STORAGE_BASE is required'); process.exit(1); }")
sub(r"^const CDN_BASE = '[^']*';", "const CDN_BASE = process.env.S5_PROXY_CDN_BASE;\nif (!CDN_BASE) { console.error('S5_PROXY_CDN_BASE is required'); process.exit(1); }")
sub(r"\{ method: 'HEAD', timeout: 10000 \}", "{ method: 'HEAD', timeout: 10000, headers: STORAGE_HOST ? { Host: STORAGE_HOST } : {} }")
sub(r"server\.listen\(PORT, '0\.0\.0\.0',", "server.listen(PORT, process.env.S5_PROXY_BIND || '127.0.0.1',")
open('/opt/s5-proxy/server.js', 'w').write(s)
print('patched OK')
