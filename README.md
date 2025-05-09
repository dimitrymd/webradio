I'll create a comprehensive README file with deployment instructions for setting up our Rust Web Radio with NGINX as a reverse proxy.

```markdown
# Rust Web Radio

A high-performance, multi-user web radio server built with Rust and the Rocket framework. This application allows you to stream your MP3 collection as an internet radio station that can handle tens of thousands of simultaneous listeners.

## Features

- **High Performance**: Built with Rust for maximum efficiency and minimal resource usage
- **Multi-User Support**: Efficiently handles thousands of simultaneous listeners
- **User-Friendly Interface**: Simple web interface for listening and playlist management
- **Playlist Management**: Automatic MP3 scanning, playlist shuffling, and track management
- **Metadata Support**: Displays track information from ID3 tags
- **Resource Efficient**: Shared audio buffering system optimized for RAM usage

## Requirements

- Rust (nightly)
- 4GB+ RAM (for high listener counts)
- Linux server (recommended for production deployment)
- NGINX (for reverse proxy)

## Architecture
```
┌─────────────────┐
│                 │
│  Music Files    │
│                 │
└────────┬────────┘
         │
         │ Reads
         ▼
┌─────────────────┐          ┌─────────────────┐
│                 │  Writes  │                 │
│  Track Player   │────────▶│  Stream Buffer  │
│   (Single)      │          │  (Shared)      │
└─────────────────┘          └────────┬────────┘
                                      │
                                      │ Reads
                                      ▼
          ┌───────────────────────────────────────────┐
          │                                           │
          │             Stream Proxy                  │
          │                                           │
          └─┬─────────────────┬────────────────────┬──┘
            │                 │                    │
            │Serves           │Serves              │Serves
            ▼                 ▼                    ▼
  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
  │                 │ │                 │ │                 │
  │   Listener 1    │ │   Listener 2    │ │   Listener N    │
  │                 │ │                 │ │                 │
  └─────────────────┘ └─────────────────┘ └─────────────────┘
```

## Quick Start

1. **Clone the repository**:
   ```bash
   git clone https://github.com/yourusername/rust-web-radio.git
   cd rust-web-radio
   ```

2. **Install Rust (nightly)**:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup default nightly
   ```

3. **Create music directory and add MP3 files**:
   ```bash
   mkdir music
   # Copy your MP3 files to the music directory
   ```

4. **Build and run for development**:
   ```bash
   cargo run --release
   ```

5. **Access the web interface**:
   Open `http://localhost:8000` in your browser.

## Production Deployment with NGINX

For production environments, we recommend running the application behind NGINX as a reverse proxy to handle SSL termination, compression, and additional security features.

### Step 1: Build the application

```bash
cargo build --release
```

### Step 2: Create a systemd service

Create a file named `/etc/systemd/system/webradio.service`:

```ini
[Unit]
Description=Rust Web Radio
After=network.target

[Service]
User=webradio
Group=webradio
WorkingDirectory=/opt/webradio
ExecStart=/opt/webradio/target/release/webradio
Restart=always
RestartSec=5
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=webradio
Environment=ROCKET_ENV=production

[Install]
WantedBy=multi-user.target
```

### Step 3: Create a dedicated user

```bash
sudo useradd -r -s /bin/false webradio
sudo mkdir -p /opt/webradio
sudo cp -r target/release /opt/webradio/
sudo cp -r static templates music /opt/webradio/
sudo chown -R webradio:webradio /opt/webradio
```

### Step 4: Install and configure NGINX

```bash
sudo apt update
sudo apt install nginx
```

Create an NGINX configuration file at `/etc/nginx/sites-available/webradio`:

```nginx
server {
    listen 80;
    server_name radio.yourdomain.com;
    
    # Redirect HTTP to HTTPS
    return 301 https://$host$request_uri;
}

server {
    listen 443 ssl;
    server_name radio.yourdomain.com;
    
    # SSL Configuration
    ssl_certificate /etc/letsencrypt/live/radio.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/radio.yourdomain.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_prefer_server_ciphers on;
    ssl_ciphers ECDHE-RSA-AES256-GCM-SHA512:DHE-RSA-AES256-GCM-SHA512:ECDHE-RSA-AES256-GCM-SHA384:DHE-RSA-AES256-GCM-SHA384;
    
    # Security headers
    add_header X-Frame-Options "SAMEORIGIN";
    add_header X-XSS-Protection "1; mode=block";
    add_header X-Content-Type-Options "nosniff";
    
    # Logging
    access_log /var/log/nginx/radio.access.log;
    error_log /var/log/nginx/radio.error.log;
    
    # Proxy settings for the Rocket server
    location / {
        proxy_pass http://127.0.0.1:8000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    
    # Special settings for the audio stream
    location /stream {
        proxy_pass http://127.0.0.1:8000;
        proxy_http_version 1.1;
        proxy_set_header Connection '';
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        
        # Specific stream settings
        proxy_buffering off;       # Disable buffering for streaming
        proxy_cache off;           # Disable caching for streaming
        proxy_read_timeout 36000s; # Increase timeout for long connections
        
        # CORS headers if needed
        add_header 'Access-Control-Allow-Origin' '*';
        add_header 'Access-Control-Allow-Methods' 'GET, OPTIONS';
    }
    
    # Static files
    location /static {
        proxy_pass http://127.0.0.1:8000;
        proxy_cache_valid 200 7d;  # Cache static assets for 7 days
        expires 7d;
        add_header Cache-Control "public, max-age=604800";
    }
}
```

Enable the site:

```bash
sudo ln -s /etc/nginx/sites-available/webradio /etc/nginx/sites-enabled/
sudo nginx -t  # Test the configuration
sudo systemctl restart nginx
```

### Step 5: Set up SSL with Let's Encrypt

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d radio.yourdomain.com
```

### Step 6: Start the web radio service

```bash
sudo systemctl daemon-reload
sudo systemctl enable webradio
sudo systemctl start webradio
```

### Step 7: Optimize for high listener counts

Edit `/etc/sysctl.conf` and add:

```
# Network settings for high concurrent connections
net.core.somaxconn = 1024
net.core.netdev_max_backlog = 5000
net.ipv4.tcp_max_syn_backlog = 8096
net.ipv4.tcp_slow_start_after_idle = 0
net.ipv4.tcp_tw_reuse = 1
net.ipv4.ip_local_port_range = 1024 65535
```

Apply changes:

```bash
sudo sysctl -p
```

Adjust system limits by editing `/etc/security/limits.conf`:

```
webradio soft nofile 65535
webradio hard nofile 65535
```

Apply the changes and restart the service:

```bash
sudo systemctl restart webradio
```

## NGINX Worker Settings

For high-traffic scenarios, optimize NGINX worker settings in `/etc/nginx/nginx.conf`:

```nginx
user www-data;
worker_processes auto;               # Set to auto or number of CPU cores
worker_rlimit_nofile 65535;          # Must be less than system limit
pid /run/nginx.pid;
include /etc/nginx/modules-enabled/*.conf;

events {
    worker_connections 10000;        # Max connections per worker
    multi_accept on;                 # Accept as many connections as possible
    use epoll;                       # Use efficient connection method on Linux
}

http {
    # Basic settings
    sendfile on;
    tcp_nopush on;
    tcp_nodelay on;
    keepalive_timeout 65;
    types_hash_max_size 2048;
    
    # Buffer sizes
    client_body_buffer_size 128k;
    client_max_body_size 10m;
    client_body_timeout 12;
    client_header_timeout 12;
    send_timeout 10;
    
    # Output buffering
    output_buffers 1 32k;
    postpone_output 1460;
    
    # Compression
    gzip on;
    gzip_vary on;
    gzip_proxied any;
    gzip_comp_level 6;
    gzip_types text/plain text/css application/json application/javascript text/xml application/xml application/xml+rss text/javascript;
    
    # Include other configs
    include /etc/nginx/conf.d/*.conf;
    include /etc/nginx/sites-enabled/*;
}
```

## Monitoring

Monitor your application's performance with:

```bash
sudo journalctl -u webradio -f  # View service logs
htop                            # Monitor CPU and RAM usage
netstat -tuln                   # View active connections
```

## Troubleshooting

1. **Service won't start**:
   - Check logs: `sudo journalctl -u webradio -n 100`
   - Verify permissions: `ls -la /opt/webradio`

2. **Cannot access the web interface**:
   - Check NGINX configuration: `sudo nginx -t`
   - Verify NGINX is running: `sudo systemctl status nginx`
   - Check firewall: `sudo ufw status`

3. **Audio streaming issues**:
   - Verify MP3 files exist in the music directory
   - Check file permissions: `sudo chown -R webradio:webradio /opt/webradio/music`

## License

MIT License

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
```

This README provides comprehensive instructions for deploying your Rust Web Radio application behind NGINX for production use. It includes detailed steps for system optimization, security configuration, and troubleshooting to ensure your streaming server can handle large numbers of concurrent listeners efficiently.