#!/bin/sh
# Toolforge helper — replacement for `make` on the bastion, where make is not
# installed. Run as the orthonaut tool (after `become orthonaut`).
#
# Usage: ./toolforge.sh <command>
#   build    - Build the container image from GitHub
#   start    - Start the web service (mounts NFS, 2Gi RAM)
#   restart  - Restart the running web service
#   stop     - Stop the web service
#   logs     - Show web service logs

set -eu

REPO="https://github.com/JavierMonton/orthonaut"

usage() {
	echo "Usage: $0 {build|start|restart|stop|logs}"
}

case "${1:-help}" in
	build)
		toolforge build start "$REPO"
		;;
	start)
		toolforge webservice buildservice start --mount all --mem 2Gi --cpu 1
		;;
	restart)
		toolforge webservice restart
		;;
	stop)
		toolforge webservice buildservice stop
		;;
	logs)
		toolforge webservice logs
		;;
	help|-h|--help)
		usage
		;;
	*)
		echo "Unknown command: $1" >&2
		usage >&2
		exit 1
		;;
esac
