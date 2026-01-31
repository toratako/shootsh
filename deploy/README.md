# Deploy

You are now in `deploy`.  

## SSH Server

```shell
# create user
sudo useradd --system --home-dir /var/lib/shootsh --shell /usr/sbin/nologin shootsh

# create directories for config and db
sudo mkdir -p /etc/shootsh /var/lib/shootsh

# copy application
sudo cp shootsh_ssh /usr/local/bin/
sudo chmod 755 /usr/local/bin/shootsh_ssh

# copy env
sudo cp .env.example /etc/shootsh/env

# create key
sudo ssh-keygen -t ed25519 -f /etc/shootsh/ssh_host_ed25519_key -N ""

# permission (chown first)
sudo chown -R shootsh:shootsh /etc/shootsh /var/lib/shootsh
sudo chmod 700 /etc/shootsh /var/lib/shootsh
sudo chmod 600 /etc/shootsh/env /etc/shootsh/ssh_host_ed25519_key

# systemd
sudo cp systemd/shootsh.service /etc/systemd/system/
sudo chmod 644 /etc/systemd/system/shootsh.service

# start
sudo systemctl daemon-reload
sudo systemctl enable --now shootsh
```

## HTML Generator

```shell
# create directory
sudo mkdir -p /var/www/shootsh

# copy template
sudo cp www/index_template.html /var/www/shootsh/

# permission
sudo chgrp shootsh /var/www
sudo chmod g+x /var/www
sudo chown -R shootsh:shootsh /var/www/shootsh/

# copy script
sudo cp www/generate_html.sh /usr/local/bin/generate_shootsh_html.sh
sudo chmod 755 /usr/local/bin/generate_shootsh_html.sh

# systemd
sudo cp systemd/shootsh-gen-html.service /etc/systemd/system/
sudo cp systemd/shootsh-gen-html.timer /etc/systemd/system/
sudo chmod 644 /etc/systemd/system/shootsh-gen-html.service /etc/systemd/system/shootsh-gen-html.timer

# start
sudo systemctl daemon-reload
sudo systemctl enable --now shootsh-gen-html.timer
```

### Web Server

```shell
# install caddy
sudo pacman -S caddy

# copy settings
sudo cp www/Caddyfile /etc/caddy/Caddyfile

# permission
sudo usermod -aG shootsh caddy
sudo chmod -R g+rX /var/www/shootsh

# firewall
sudo firewall-cmd --zone=public --permanent --add-port=80/tcp
sudo firewall-cmd --zone=public --permanent --add-port=443/tcp

# start
sudo systemctl reload firewalld.service
sudo systemctl enable --now caddy
```
