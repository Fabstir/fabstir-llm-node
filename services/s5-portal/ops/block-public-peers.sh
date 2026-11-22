#!/usr/bin/env bash
# Refuse an S5 portal node's outbound connections to public S5 peers.
#
# Why this exists: an S5 node that has EVER peered publicly remembers those peers in its database and redials them
# on its own, even when its configured peer list is empty (`[p2p.peers] initial = []`). Configuration alone therefore
# does not isolate a portal. This script refuses the node's outbound TLS connections to the remembered hosts by name
# (the name in the TLS hello), so no other traffic is touched: the storage backend, Caddy and certificate renewal are
# unaffected. It is a stopgap until the remembered peers are purged at the next planned node restart.
#
# Usage (as root):   s5-block-public-peers.sh          add the rules (idempotent: running it twice adds nothing)
#                    s5-block-public-peers.sh undo     remove every rule this script added
# Check it worked:   iptables -S OUTPUT | grep s5-block-public-peers
# Verify isolation by watching live connections (ss -tn state established), not by reading configuration.
# Installed as /usr/local/sbin/s5-block-public-peers.sh, run at boot by s5-block-public-peers.service.
set -euo pipefail

# EDIT HERE: the public peers to refuse, space separated, exactly as they appear in the TLS hello.
HOSTS="s5.serey.io s5.ninja s5.garden fabstir.s5net.work node.sfive.net s5.cx s5.vup.cx"

# EDIT HERE: hosts that also have a dedicated address, refused by address as well. This ends a link that is already
# open (a name rule only stops new ones). Format: "name=address", space separated. Leave empty for none.
DEDICATED="s5.vup.cx=188.245.99.196"

for t in iptables ip6tables; do
  for h in $HOSTS; do
    R="-p tcp --dport 443 -m string --string $h --algo bm -m comment --comment s5-block-public-peers -j REJECT --reject-with tcp-reset"
    if [[ "${1:-}" == undo ]]; then while $t -C OUTPUT $R 2>/dev/null; do $t -D OUTPUT $R; done
    else $t -C OUTPUT $R 2>/dev/null || $t -I OUTPUT 1 $R; fi
  done
done
# Dedicated addresses are IPv4 only here.
for pair in $DEDICATED; do
  V="-d ${pair#*=} -p tcp --dport 443 -m comment --comment s5-block-public-peers -j REJECT --reject-with tcp-reset"
  if [[ "${1:-}" == undo ]]; then while iptables -C OUTPUT $V 2>/dev/null; do iptables -D OUTPUT $V; done
  else iptables -C OUTPUT $V 2>/dev/null || iptables -I OUTPUT 1 $V; fi
done
