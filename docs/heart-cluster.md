# Heart in the target cluster

Heart is an internal dependency for the optional event-planning pilot. Ordinary
chat does not depend on it. Deploy Heart only after its reviewed ARM64 image and
digest have been recorded, and keep the API private to the `sproyt` namespace.

These commands assume the existing PostgreSQL service and administrator Secret
described in [running.md](running.md): namespace `database`, Service
`postgres-postgresql`, Secret `postgres-postgresql`, key `postgres-password`.

## Create the Heart database identity

Run this through the cluster's web SSH session. It creates a dedicated login and
database; the application never receives the PostgreSQL administrator password.

```sh
kubectl create namespace heart --dry-run=client -o yaml | kubectl apply -f -

HEART_DB_PASSWORD="$(openssl rand -hex 32)"
kubectl -n database create secret generic heart-db-bootstrap \
  --from-literal=app-password="$HEART_DB_PASSWORD"

cat <<'YAML' | kubectl apply -f -
apiVersion: v1
kind: Pod
metadata:
  name: heart-db-bootstrap
  namespace: database
spec:
  restartPolicy: Never
  containers:
    - name: psql
      image: postgres:17-alpine
      command: ["sleep", "3600"]
      env:
        - name: PGPASSWORD
          valueFrom:
            secretKeyRef:
              name: postgres-postgresql
              key: postgres-password
        - name: APP_PASSWORD
          valueFrom:
            secretKeyRef:
              name: heart-db-bootstrap
              key: app-password
YAML

kubectl -n database wait --for=condition=Ready pod/heart-db-bootstrap --timeout=120s
kubectl -n database exec -i heart-db-bootstrap -- sh -c \
  'psql -h postgres-postgresql -U postgres -d postgres -v ON_ERROR_STOP=1 -v app_password="$APP_PASSWORD"' <<'SQL'
select format('create role heart login password %L', :'app_password')
where not exists (select from pg_roles where rolname = 'heart') \gexec
select format('alter role heart password %L', :'app_password') \gexec
select 'create database heart owner heart'
where not exists (select from pg_database where datname = 'heart') \gexec
SQL

kubectl -n heart create secret generic heart \
  --from-literal=DATABASE_URL="postgresql://heart:${HEART_DB_PASSWORD}@postgres-postgresql.database.svc.cluster.local:5432/heart" \
  --dry-run=client -o yaml | kubectl apply -f -

kubectl -n database delete pod heart-db-bootstrap --wait=true
kubectl -n database delete secret heart-db-bootstrap
unset HEART_DB_PASSWORD
```

Copy the existing registry pull credential into the namespace:

```sh
kubectl -n database get secret oci-pull-secret -o json \
  | jq 'del(.metadata.creationTimestamp,.metadata.resourceVersion,.metadata.uid,.metadata.managedFields) | .metadata.namespace="heart"' \
  | kubectl apply -f -
```

## Migrate and deploy

Use the reviewed ARM64 digest recorded in
[roadmap-status.md](roadmap-status.md), never a mutable tag. It corresponds to
Heart PR 3 revision `6d8a3b50952e866e6500b5d325afeb03d3f3c7d7`; merge or otherwise approve
that PR before treating the candidate as a production release.

```sh
HEART_IMAGE='oci.bjoroy.me/sproyt/heart@sha256:2d244cf57cdaff1c18b21737d4d05b1a7b935ecf1d60cb3cdd337435bc3142cb'

cat > /tmp/heart-migrate.yaml <<EOF
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: heart
  namespace: heart
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: heart
  policyTypes: [Ingress, Egress]
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: sproyt
          podSelector:
            matchLabels:
              app.kubernetes.io/name: sproyt
      ports:
        - {protocol: TCP, port: 3000}
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: kube-system
      ports:
        - {protocol: UDP, port: 53}
        - {protocol: TCP, port: 53}
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: database
      ports:
        - {protocol: TCP, port: 5432}
---
apiVersion: batch/v1
kind: Job
metadata:
  name: heart-migrate
  namespace: heart
spec:
  backoffLimit: 2
  ttlSecondsAfterFinished: 600
  template:
    metadata:
      labels:
        app.kubernetes.io/name: heart
    spec:
      restartPolicy: Never
      automountServiceAccountToken: false
      imagePullSecrets:
        - name: oci-pull-secret
      containers:
        - name: migrate
          image: ${HEART_IMAGE}
          args: ["migrate"]
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: heart
                  key: DATABASE_URL
EOF
kubectl apply -f /tmp/heart-migrate.yaml
kubectl -n heart wait --for=condition=Complete job/heart-migrate --timeout=180s
kubectl -n heart logs job/heart-migrate
```

Apply the private API, disruption budget and default-deny network policy:

```sh
cat > /tmp/heart.yaml <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: heart
  namespace: heart
spec:
  replicas: 2
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 0
      maxSurge: 1
  selector:
    matchLabels:
      app.kubernetes.io/name: heart
  template:
    metadata:
      labels:
        app.kubernetes.io/name: heart
    spec:
      automountServiceAccountToken: false
      terminationGracePeriodSeconds: 30
      imagePullSecrets:
        - name: oci-pull-secret
      securityContext:
        runAsNonRoot: true
        runAsUser: 65532
        runAsGroup: 65532
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: heart
          image: ${HEART_IMAGE}
          ports:
            - name: http
              containerPort: 3000
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: heart
                  key: DATABASE_URL
          readinessProbe:
            httpGet: {path: /ready, port: http}
            periodSeconds: 5
          livenessProbe:
            httpGet: {path: /health, port: http}
            periodSeconds: 10
          resources:
            requests: {cpu: 25m, memory: 48Mi}
            limits: {cpu: 500m, memory: 256Mi}
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities: {drop: ["ALL"]}
---
apiVersion: v1
kind: Service
metadata:
  name: heart
  namespace: heart
spec:
  selector:
    app.kubernetes.io/name: heart
  ports:
    - name: http
      port: 3000
      targetPort: http
---
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: heart
  namespace: heart
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app.kubernetes.io/name: heart
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: heart
  namespace: heart
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: heart
  policyTypes: [Ingress, Egress]
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: sproyt
          podSelector:
            matchLabels:
              app.kubernetes.io/name: sproyt
      ports:
        - {protocol: TCP, port: 3000}
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: kube-system
      ports:
        - {protocol: UDP, port: 53}
        - {protocol: TCP, port: 53}
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: database
      ports:
        - {protocol: TCP, port: 5432}
EOF

kubectl apply -f /tmp/heart.yaml
kubectl -n heart rollout status deployment/heart --timeout=180s
kubectl -n heart get pods,service,pdb,networkpolicy
```

The Sproyt Helm values then use `heartUrl:
http://heart.heart.svc.cluster.local:3000` and the Heart namespace/pod selectors
shown in [running.md](running.md). Do not create an Ingress for Heart.

## Rolling-update acceptance

After Sproyt and Heart are connected, start one event-planning process, restart
one Sproyt pod while its durable outbox item is leased, and roll both
Deployments. Verify:

```sh
kubectl -n sproyt rollout restart deployment/sproyt-sproyt
kubectl -n sproyt rollout status deployment/sproyt-sproyt --timeout=180s
kubectl -n heart rollout restart deployment/heart
kubectl -n heart rollout status deployment/heart --timeout=180s
```

The process view must contain exactly one `process.started` event and continue
to completion. Send an ordinary chat message while Heart is scaled to zero; the
message must still persist and reach the other participant. Restore Heart and
confirm the queued process operation completes without a duplicate instance.
