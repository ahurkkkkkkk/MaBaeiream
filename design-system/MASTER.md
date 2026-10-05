# MaBaeiream Design System

> One shared visual language for the native Android and desktop apps. Android follows the device appearance; desktop has a compact Light/Night control. Keep the semantic roles below aligned with `AppPalette` in Android and `AppTheme` in Slint.

**Product:** private media library and watch room for two  
**Audience:** two partners sharing videos, calls, and a private file shelf  
**Mood:** cozy, affectionate, calm, and dependable  
**Direction:** warm soft-SaaS with a quiet editorial touch  
**Motion:** restrained 160–200 ms feedback; no layout-shifting hover effects

## Color tokens

| Role | Light | Dark | Use |
|---|---|---|---|
| `canvas` | `#F5F8F2` | `#161D17` | App background |
| `surface` | `#FFFEFB` | `#202A23` | Main cards and inputs |
| `surface-soft` | `#EDF3EC` | `#29372D` | Secondary cards, navigation |
| `surface-rose` | `#F6E5E9` | `#3C2931` | Couple, voice, and shared-room moments |
| `ink` | `#23362D` | `#F1F3EF` | Main text |
| `muted` | `#58695F` | `#BBC7BD` | Body copy and supporting labels |
| `muted-soft` | `#626F65` | `#98A69B` | Secondary metadata |
| `line` | `#DCE5DA` | `#39483D` | Borders and dividers |
| `primary` | `#286B50` | `#8CCBA5` | Main actions, selected navigation, progress |
| `on-primary` | `#FFFFFF` | `#183B27` | Text and icons on primary |
| `accent` | `#924359` | `#F1A7B8` | Couple identity and links |
| `accent-ink` | `#873C53` | `#F3BECA` | Text on soft-rose surfaces |
| `success` | `#286B50` | `#8CCBA5` | Connected and ready states |
| `danger` | `#983D4C` | `#F1A5B0` | Errors, decline, and hang-up actions |
| `danger-soft` | `#F8E6E8` | `#42282E` | Error backgrounds |

Use pink to signal the shared relationship and green for navigation, readiness, and forward actions. Avoid introducing blue or yellow as extra product accents. Keep media playback itself black and immersive.

## Type and spacing

- Use platform sans for controls and body text; use the platform serif for short page titles and the home feature line.
- Keep small utility labels at 10–12 sp/px and body copy at 13–16 sp/px. Avoid long all-caps copy.
- Use a 4/8/12/16/24/32 spacing rhythm. Android touch targets are at least 48 dp; desktop controls are at least 36 px tall.
- Use 12–16 dp/px radii for controls and 20–26 for feature cards. Reserve the largest shapes for the couple hero and call card.

## Screen patterns

- **Sign-in:** one clear credential form with the secure-server address visible, a strong green submit action, readable errors, and a short privacy reassurance.
- **Home:** greeting and room state first; one prominent route into the shared watch room; call controls and recent files follow.
- **Files:** folder path and search stay visible; upload and create-folder actions are easy to find; file rows use clear type and size labels.
- **Downloads:** distinguish saving a link from playing one together; show progress and keep cancellation visible while a transfer runs.
- **Watch room:** shared playback and room state first; voice call and live-stream controls remain separate and understandable.
- **Desktop:** persistent left navigation and a spacious content canvas. **Android:** native stacked content and bottom navigation; no scaled-down desktop layout.

## Interaction and accessibility

- Preserve the existing playback, call, upload, folder, download, and sign-out behavior.
- Keep hover, pressed, focus, selected, disabled, loading, and error feedback visible where applicable.
- Use descriptive accessible labels for fields and icon-only controls. Color never carries a state by itself.
- Maintain text contrast of at least 4.5:1 and focus indicators with clear separation from the component.
- Respect the device reduced-motion setting. Animation is feedback only and never delays a control.

## Avoid

- Mixed blue/yellow accent systems, pastel text on pastel fills, decorative-only navigation, and emoji used as icons.
- Repeated identical cards when the content has a clear priority; use one featured room card and quieter utility sections.
- Playful labels that obscure an action, status, or error.
