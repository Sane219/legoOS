{{/*
Base chart name.
*/}}
{{- define "legoos.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Fully qualified app name, e.g. "legoos-api".
*/}}
{{- define "legoos.fullname" -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Chart name and version, for the chart label.
*/}}
{{- define "legoos.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Common labels, scoped to one component. Always call with a dict:
{{ include "legoos.labels" (dict "root" $ "component" "api") }}
*/}}
{{- define "legoos.labels" -}}
helm.sh/chart: {{ include "legoos.chart" .root }}
{{ include "legoos.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .root.Release.Service }}
{{- end -}}

{{/*
Selector labels, scoped to one component. Always call with a dict:
{{ include "legoos.selectorLabels" (dict "root" $ "component" "api") }}
*/}}
{{- define "legoos.selectorLabels" -}}
app.kubernetes.io/name: {{ include "legoos.name" .root }}
app.kubernetes.io/instance: {{ .root.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{/*
Component-scoped fullname, e.g. "legoos-api".
*/}}
{{- define "legoos.componentName" -}}
{{- printf "%s-%s" (include "legoos.fullname" .root) .component | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Name of the Secret holding JWT_SECRET / MCP_CREDENTIAL_KEY / ANTHROPIC_API_KEY / VOYAGE_API_KEY.
*/}}
{{- define "legoos.secretName" -}}
{{- if .Values.secrets.existingSecret -}}
{{- .Values.secrets.existingSecret -}}
{{- else -}}
{{- include "legoos.fullname" . -}}-secrets
{{- end -}}
{{- end -}}

{{/*
Postgres host (bundled StatefulSet service, or global override for a managed DB).
*/}}
{{- define "legoos.postgresHost" -}}
{{- .Values.global.postgresHost | default (printf "%s-postgres" (include "legoos.fullname" .)) -}}
{{- end -}}

{{/*
Redis host.
*/}}
{{- define "legoos.redisHost" -}}
{{- .Values.global.redisHost | default (printf "%s-redis" (include "legoos.fullname" .)) -}}
{{- end -}}

{{/*
Qdrant host.
*/}}
{{- define "legoos.qdrantHost" -}}
{{- .Values.global.qdrantHost | default (printf "%s-qdrant" (include "legoos.fullname" .)) -}}
{{- end -}}
