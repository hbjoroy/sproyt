{{- define "sproyt.name" -}}sproyt{{- end }}
{{- define "sproyt.fullname" -}}{{ .Release.Name }}-sproyt{{- end }}
{{- define "sproyt.labels" -}}
app.kubernetes.io/name: {{ include "sproyt.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "sproyt.image" -}}
{{- if and (eq .Values.config.environment "production") (not .Values.image.digest) -}}
{{- fail "image.digest is required when config.environment=production" -}}
{{- end -}}
{{- if .Values.image.digest -}}
{{ printf "%s@%s" .Values.image.repository .Values.image.digest }}
{{- else -}}
{{ printf "%s:%s" .Values.image.repository .Values.image.tag }}
{{- end -}}
{{- end }}

{{- define "sproyt.migrationImage" -}}
{{- $repository := default .Values.image.repository .Values.migration.image.repository -}}
{{- $tag := default .Values.image.tag .Values.migration.image.tag -}}
{{- $digest := default .Values.image.digest .Values.migration.image.digest -}}
{{- if and (eq .Values.config.environment "production") (not $digest) -}}
{{- fail "migration.image.digest or image.digest is required when config.environment=production" -}}
{{- end -}}
{{- if $digest -}}
{{ printf "%s@%s" $repository $digest }}
{{- else -}}
{{ printf "%s:%s" $repository $tag }}
{{- end -}}
{{- end }}

{{- define "sproyt.heart.fullname" -}}{{ include "sproyt.fullname" . }}-heart{{- end }}
{{- define "sproyt.heart.labels" -}}
app.kubernetes.io/name: heart
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: sproyt
{{- end }}
{{- define "sproyt.heart.image" -}}
{{- if and (eq .Values.config.environment "production") (not .Values.heart.image.digest) -}}
{{- fail "heart.image.digest is required when heart.enabled=true in production" -}}
{{- end -}}
{{- if .Values.heart.image.digest -}}
{{ printf "%s@%s" .Values.heart.image.repository .Values.heart.image.digest }}
{{- else -}}
{{ printf "%s:%s" .Values.heart.image.repository .Values.heart.image.tag }}
{{- end -}}
{{- end }}
