FROM nginxinc/nginx-unprivileged:1.29-alpine

COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY public /usr/share/nginx/html
COPY admin /usr/share/nginx/admin

EXPOSE 8080
