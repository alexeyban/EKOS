{{- define "ekos.name" -}}{{ .Release.Name | trunc 50 | trimSuffix "-" }}{{- end -}}

{{- define "ekos.labels" -}}
app.kubernetes.io/name: ekos
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Values.image.tag | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}

{{- define "ekos.selector" -}}
app.kubernetes.io/name: ekos
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "ekos.serviceAccount" -}}
{{- if .Values.serviceAccount.create -}}
{{ default (include "ekos.name" .) .Values.serviceAccount.name }}
{{- else -}}
{{ required "serviceAccount.name is required when serviceAccount.create is false" .Values.serviceAccount.name }}
{{- end -}}
{{- end -}}

{{- define "ekos.claim" -}}
{{ default (printf "%s-data" (include "ekos.name" .)) .Values.persistence.existingClaim }}
{{- end -}}
