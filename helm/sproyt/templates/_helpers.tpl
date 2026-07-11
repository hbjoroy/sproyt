{{- define "sproyt.name" -}}sproyt{{- end }}
{{- define "sproyt.fullname" -}}{{ .Release.Name }}-sproyt{{- end }}
{{- define "sproyt.labels" -}}
app.kubernetes.io/name: {{ include "sproyt.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}
