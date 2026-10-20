/** Form-row styles shared by Settings → AI and the Research page (same look as the settings page). */
export const FORM_STYLES = `
  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-6);
    padding-block: var(--space-4);
  }

  .setting + .setting {
    border-block-start: 1px solid var(--color-border);
  }

  .setting.column {
    flex-direction: column;
    align-items: stretch;
    gap: var(--space-3);
  }

  .setting-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .setting-name {
    font-weight: var(--weight-medium);
  }

  .setting-desc {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
  }

  .input {
    inline-size: 100%;
    block-size: 36px;
    padding-inline: 12px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-sm);
  }

  .input.is-invalid {
    border-color: var(--color-danger);
  }

  .input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-ring);
  }

  .input::placeholder {
    color: var(--color-text-muted);
  }

  .field-error {
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .toggle-row {
    cursor: pointer;
  }

  .switch {
    position: relative;
    flex-shrink: 0;
    inline-size: 40px;
    block-size: 22px;
    border-radius: var(--radius-full);
    background: var(--color-surface-3);
    border: 1px solid var(--color-border-strong);
    transition: background-color var(--motion-base) var(--ease-standard);
  }

  .switch input {
    position: absolute;
    inset: 0;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .knob {
    position: absolute;
    inset-block-start: 2px;
    inset-inline-start: 2px;
    inline-size: 16px;
    block-size: 16px;
    border-radius: var(--radius-full);
    background: var(--color-text-muted);
    transition:
      transform var(--motion-base) var(--ease-emphasized),
      background-color var(--motion-base) var(--ease-standard);
  }

  .switch.is-on {
    background: var(--color-accent);
    border-color: transparent;
  }

  .switch.is-on .knob {
    background: var(--color-on-accent);
    transform: translateX(calc(var(--dir-sign) * 18px));
  }

  .switch:has(input:focus-visible) {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .dot {
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-full);
    background: var(--color-text-muted);
    flex-shrink: 0;
  }

  .dot.is-on {
    background: var(--color-success);
    box-shadow: 0 0 0 3px var(--state-completed-soft);
  }
`;
