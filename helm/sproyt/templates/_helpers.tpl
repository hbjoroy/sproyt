{{- define "sproyt.name" -}}sproyt{{- end }}
{{- define "sproyt.fullname" -}}{{ .Release.Name }}-sproyt{{- end }}
{{- define "sproyt.labels" -}}
app.kubernetes.io/name: {{ include "sproyt.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "sproyt.image" -}}
{{- if .Values.image.digest -}}
{{ printf "%s@%s" .Values.image.repository .Values.image.digest }}
{{- else -}}
{{ printf "%s:%s" .Values.image.repository .Values.image.tag }}
{{- end -}}
{{- end }}
