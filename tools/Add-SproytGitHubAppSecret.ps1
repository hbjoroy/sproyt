# Import an existing GitHub-generated key; never generate or print credentials.
# GitHub registration/installation is performed by the owner in GitHub first.
[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)][ValidatePattern('^[1-9][0-9]*$')][string]$AppId,
    [Parameter(Mandatory)][string]$PrivateKeyPath,
    [ValidateSet('sproyt-canary')][string]$Namespace = 'sproyt-canary',
    [ValidatePattern('^[a-z0-9][a-z0-9-]*[a-z0-9]$')][string]$SecretName = 'sproyt-github-app'
)
$ErrorActionPreference = 'Stop'
$taskKeyPath = (Resolve-Path -LiteralPath $PrivateKeyPath).Path
if (-not (Test-Path -LiteralPath $taskKeyPath -PathType Leaf)) { throw 'Private key path must be a file.' }
$taskRsa = [System.Security.Cryptography.RSA]::Create()
try {
    $taskPem = [IO.File]::ReadAllText($taskKeyPath)
    try { $taskRsa.ImportFromPem($taskPem) } catch { throw 'The file is not a supported RSA private key.' }
    if ($taskRsa.KeySize -lt 2048) { throw 'RSA key must be at least 2048 bits.' }
    # Confirm this is a private key, without logging or retaining its parameters.
    try { $null = $taskRsa.ExportParameters($true) } catch { throw 'The file does not contain a private key.' }
} finally {
    $taskPem = $null
    $taskRsa.Dispose()
}
if (-not $PSCmdlet.ShouldProcess("default/$Namespace/$SecretName", 'Import the existing GitHub App credential')) { return }

# Capture credential-bearing JSON and pipe it directly to the API. Do not save
# it to disk or let PowerShell render it as command output. Use a dedicated
# Secret, so database/session credentials are never rewritten by this helper.
$taskSecretJson = & kubectl --context default --request-timeout=20s create secret generic $SecretName `
    --namespace $Namespace --from-literal "app-id=$AppId" --from-file "private-key.pem=$taskKeyPath" `
    --dry-run=client -o json
if ($LASTEXITCODE -ne 0) { throw 'Could not prepare the GitHub App Secret.' }
try {
    # Server-side apply does not duplicate the private key in last-applied annotations.
    $taskSecretJson | & kubectl --context default --request-timeout=20s apply --server-side `
        --field-manager=sproyt-github-setup --namespace $Namespace -f -
    if ($LASTEXITCODE -ne 0) { throw 'GitHub App Secret import failed.' }
} finally {
    $taskSecretJson = $null
}
Write-Output "GitHub App credential stored in $Namespace/$SecretName. No application rollout or export was enabled."
