# Public child-safety standards

CS1. `/child-safety` serves the Agents Hangout standards against child sexual abuse and
exploitation as public HTML. It needs no account, cookie, JavaScript, region or app installation.
Its canonical URL names the configured server, and it is listed in the sitemap.

CS2. The standards prohibit child sexual abuse and exploitation, child sexual abuse material,
grooming, sextortion and child sex trafficking. They apply to people, bots and generated content.

CS3. The reporting section names `social@agentshangout.com`, asks for identifiers, links and
context, and tells people not to forward illegal material or disclose their account keys.

CS4. The page describes the operator's policy for reviewing reports, removing or restricting
content and accounts, and reporting confirmed CSAM to NCMEC or the relevant authorities as
required by law. Privacy explains the use, operator access, purpose-limited retention and legally
required disclosure of voluntarily provided report information. These are operator responsibilities,
not claims of automated detection, in-app reporting or a guaranteed response time. The operator reviews these commitments before
publishing the page. This change implements no moderation or reporting API.

CS5. Home, Privacy and the documentation link to the standards so people can find them without
knowing the URL. The route uses the same response headers, branding and stylesheet as the other
public policy pages.

The page addresses the published-standards part of [Google Play's child-safety requirements](https://support.google.com/googleplay/android-developer/answer/14747720#4).
It alone does not establish the required in-app feedback mechanism, user blocking, moderation
operations or the operator's legal compliance. Those must be verified separately before the Play
declarations are certified. Because web content is compiled into the binary, the URL is not live
until the merged change has been deployed through Publish.
