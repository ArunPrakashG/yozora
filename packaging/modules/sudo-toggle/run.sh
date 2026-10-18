#!/bin/sh
# Prints one line of Update JSON describing whether passwordless sudo is on.
# Asks sudo itself: /etc/sudoers.d is root-only, so a plain file check always fails.
if sudo -n -l 2>/dev/null | grep -q 'NOPASSWD: ALL'; then
    printf '%s\n' '{"bar":{"icon":"lock-open","tone":"warning"},"popover":{"title":"Sudo","caption":"passwordless","body":[{"type":"section","heading":"Passwordless sudo","rows":[{"type":"row","icon":"lock-open","title":"On","subtitle":"Every sudo command skips the password","badge":{"text":"On","tone":"warning"}}]},{"type":"buttons","items":[{"label":"Disable","action":"disable","primary":true}]},{"type":"note","text":"Turn it off when you are done."}]}}'
else
    printf '%s\n' '{"bar":{"icon":"lock"},"popover":{"title":"Sudo","caption":"password required","body":[{"type":"section","heading":"Passwordless sudo","rows":[{"type":"row","icon":"lock","title":"Off","subtitle":"sudo asks for your password","badge":{"text":"Off","tone":"success"}}]},{"type":"buttons","items":[{"label":"Enable","action":"enable","destructive":true}]},{"type":"note","text":"Enabling asks you to authenticate."}]}}'
fi
