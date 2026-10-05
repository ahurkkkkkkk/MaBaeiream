#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
    echo "Run this installer as root." >&2
    exit 1
fi

ENV_FILE=/etc/mabaeiream/mabaeiream.env
TURN_CONFIG=/etc/turnserver.conf
CERT_DIR=/etc/letsencrypt/live/ahura.site
TURN_CERT_DIR=/etc/coturn/mabaeiream-certs
CERT_HOOK=/etc/letsencrypt/renewal-hooks/deploy/90-mabaeiream-turn-cert

if [[ ! -f "$ENV_FILE" ]]; then
    echo "MaBaeiream environment file is missing: $ENV_FILE" >&2
    exit 1
fi
if [[ ! -r "$CERT_DIR/fullchain.pem" || ! -r "$CERT_DIR/privkey.pem" ]]; then
    echo "TLS certificate for ahura.site is missing." >&2
    exit 1
fi

if ! command -v turnserver >/dev/null 2>&1; then
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y coturn
fi

if ! id turnserver >/dev/null 2>&1; then
    echo "The coturn package did not create its turnserver account." >&2
    exit 1
fi

install -d -o root -g turnserver -m 0750 "$TURN_CERT_DIR"
install -o root -g turnserver -m 0644 "$CERT_DIR/fullchain.pem" "$TURN_CERT_DIR/fullchain.pem"
install -o root -g turnserver -m 0640 "$CERT_DIR/privkey.pem" "$TURN_CERT_DIR/privkey.pem"
if ! runuser -u turnserver -- test -r "$TURN_CERT_DIR/privkey.pem"; then
    echo "turnserver cannot read its ahura.site TLS certificate copy." >&2
    exit 1
fi

install -d -o root -g root -m 0755 "$(dirname "$CERT_HOOK")"
cat > "$CERT_HOOK" <<EOF
#!/usr/bin/env bash
set -euo pipefail
install -o root -g turnserver -m 0644 "$CERT_DIR/fullchain.pem" "$TURN_CERT_DIR/fullchain.pem"
install -o root -g turnserver -m 0640 "$CERT_DIR/privkey.pem" "$TURN_CERT_DIR/privkey.pem"
systemctl restart coturn.service
EOF
chown root:root "$CERT_HOOK"
chmod 0755 "$CERT_HOOK"

secret="$(openssl rand -hex 32)"
env_tmp="$(mktemp /etc/mabaeiream/mabaeiream.env.XXXXXX)"
trap 'rm -f "$env_tmp"' EXIT
awk '!/^(MABAEIREAM_TURN_URLS|MABAEIREAM_TURN_SECRET)=/' "$ENV_FILE" > "$env_tmp"
cat >> "$env_tmp" <<EOF
# The VPS has no routed public IPv6 address while ahura.site advertises AAAA.
# Use its stable IPv4 address for plain TURN so ICE does not depend on broken IPv6 DNS.
MABAEIREAM_TURN_URLS=turn:146.19.130.38:3478?transport=udp,turn:146.19.130.38:3478?transport=tcp,turns:ahura.site:5349?transport=tcp
MABAEIREAM_TURN_SECRET=$secret
EOF
chown root:mabaeiream "$env_tmp"
chmod 0640 "$env_tmp"
mv "$env_tmp" "$ENV_FILE"
trap - EXIT

config_tmp="$(mktemp /etc/turnserver.conf.XXXXXX)"
trap 'rm -f "$config_tmp"' EXIT
cat > "$config_tmp" <<EOF
listening-port=3478
tls-listening-port=5349
fingerprint
use-auth-secret
static-auth-secret=$secret
realm=ahura.site
server-name=ahura.site
stale-nonce=600
min-port=49160
max-port=49200
user-quota=4
total-quota=50
no-cli
no-multicast-peers
no-tlsv1
no-tlsv1_1
cert=$TURN_CERT_DIR/fullchain.pem
pkey=$TURN_CERT_DIR/privkey.pem
syslog
EOF
chown root:turnserver "$config_tmp"
chmod 0640 "$config_tmp"
mv "$config_tmp" "$TURN_CONFIG"
trap - EXIT

if [[ -f /etc/default/coturn ]] && ! grep -q '^TURNSERVER_ENABLED=1$' /etc/default/coturn; then
    sed -i '/^TURNSERVER_ENABLED=/d' /etc/default/coturn
    printf '\nTURNSERVER_ENABLED=1\n' >> /etc/default/coturn
fi

systemctl daemon-reload
systemctl enable coturn.service
systemctl restart coturn.service
systemctl restart mabaeiream.service

if command -v ufw >/dev/null 2>&1 && ufw status | grep -q '^Status: active'; then
    ufw allow 3478/tcp
    ufw allow 3478/udp
    ufw allow 5349/tcp
    ufw allow 49160:49200/udp
fi

systemctl --no-pager --full status coturn.service
systemctl --no-pager --full status mabaeiream.service
