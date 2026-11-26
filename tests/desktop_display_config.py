"""Host transport settings for the native, display-only SPICE agent."""

def configure_display(config, arguments, manifest):
    if not (manifest.get('display_agent_probe') or manifest.get('desktop_resize')):
        return
    config['Display'][0].update(DynamicResolution=True,NativeResolution=True)
    arguments.append('-machine highmem-ecam=off')
    # SPICE 0.14 routes client-mode mouse events to any connected agent.
    # Our agent handles monitor configuration only; keep input on VirtIO.
    arguments.append('-spice agent-mouse=off')
