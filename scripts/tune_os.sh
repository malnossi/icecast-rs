#!/usr/bin/env bash
# OS tuning for High-Concurrency Icecast Server

echo "Tuning OS parameters for high network concurrency..."

# Increase file descriptor limit for the current shell session
ulimit -n 50000

# Optional sysctl tuning for macOS
# Increase ephemeral port range
sudo sysctl -w net.inet.ip.portrange.first=10000
sudo sysctl -w net.inet.ip.portrange.hifirst=10000

# Increase maximum sockets
sudo sysctl -w kern.maxfiles=50000
sudo sysctl -w kern.maxfilesperproc=50000

echo "Tuning complete. Current ulimit -n is: $(ulimit -n)"
