# Security policy

Do not open a public issue for a security report.

Report vulnerabilities in vxmcp privately through GitHub's private vulnerability reporting:
open the repository's **Security** tab and choose **Report a vulnerability**
(https://github.com/vxedge/vxmcp/security/advisories/new).

In scope here: the MCP server itself, its tool surface (every tool is one vxAPI RPC), and the
human confirmation flow that gates every mutating tool. vxmcp is an agent-facing server, so it
treats two things as untrusted input: the output of any tool it returns to an agent, and the
`requestState` a client echoes back on a retry. A mutating RPC must never run without a
confirmation the user gave; any path that runs one unconfirmed, replays a confirmation, or
lets a ticket cover a different target than the RPC acts on is a vulnerability.

Supported versions, response targets, and scope notes are in the vxedge organization
security policy: https://github.com/vxedge/.github/blob/main/SECURITY.md
