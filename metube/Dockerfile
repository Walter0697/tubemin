FROM ghcr.io/alexta69/metube:latest

COPY docker-entrypoint.sh /app/tubemin-entrypoint.sh
COPY maintenance.py /app/tubemin-maintenance.py
RUN chmod +x /app/tubemin-entrypoint.sh

ENTRYPOINT ["/usr/bin/tini", "-g", "--", "/app/tubemin-entrypoint.sh"]
