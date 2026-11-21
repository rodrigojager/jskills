const easeOut = 'cubic-bezier(.23,1,.32,1)';
const recipe = (id, title, category, target, kinds, trigger, sample, duration, note, keyframes, timing = {}) => ({
  id, version: 1, title, category, target, kinds, trigger, sample, duration, note, keyframes, easing: easeOut, ...timing,
});

export const triggers = {
  click: 'Click or activate', hover: 'Hover', focus: 'Keyboard focus', success: 'Success',
  failure: 'Failure', change: 'State change', insert: 'Content inserted', open: 'Open',
  selection: 'Selection change', custom: 'Other (describe below)',
};

export const particleShapes = { spark: 'Spark', dot: 'Dot', ring: 'Ring', star: 'Star', diamond: 'Diamond' };
export const particleTrajectories = { burst: 'Spread outward', rise: 'Drift up', fall: 'Drift down' };
export const particlePresets = {
  'soft-sparks': { title: 'Soft sparks', shape: 'spark', trajectory: 'burst', count: 7, distance: 24 },
  'rising-dots': { title: 'Rising dots', shape: 'dot', trajectory: 'rise', count: 5, distance: 18 },
  'falling-confetti': { title: 'Falling confetti', shape: 'diamond', trajectory: 'fall', count: 10, distance: 30 },
};

export const motionReferences = {
  'reaction-burst': 'Local reaction burst', milestone: 'Milestone accent', 'copy-check': 'Copy → check → copy',
  'draw-check': 'Draw a completion mark', 'hold-fill': 'Hold fill with early release',
  'play-pause': 'Play/pause icon handoff', chevron: 'Directional chevron extension',
  'card-detail': 'Card → detail → card', 'settling-results': 'Results viewport settles',
  'sheet-snap': 'Sheet snaps or returns', 'pending-result': 'Pending → result in one toast',
  'action-progress': 'Progress inside the action', 'roll-digits': 'Roll changed digits',
  reorder: 'Show the reorder destination', 'selection-range': 'Preview a selection range',
};

const directions = { right: 'Right', left: 'Left', up: 'Up', down: 'Down' };
const vectors = { right: [1, 0], left: [-1, 0], up: [0, -1], down: [0, 1] };

function directionalFrames({ direction, distance }) {
  const [x, y] = vectors[direction];
  return [
    { translate: '0px 0px', easing: easeOut },
    { translate: `${x * distance}px ${y * distance}px`, offset: .42, easing: 'ease-in-out' },
    { translate: '0px 0px' },
  ];
}

function turnFrames({ degrees, direction }) {
  const sign = direction === 'clockwise' ? -1 : 1;
  return [{ rotate: `${sign * Number(degrees)}deg` }, { rotate: '0deg' }];
}

function revealFrames({ edge, extent, travel }, styles) {
  if (styles.clipPath !== 'none') return [{ opacity: .24 }, {}];
  const inset = ['bottom', 'left', 'top', 'right'].map((side) => side === edge ? extent + '%' : '0');
  const round = ' round ' + styles.borderRadius;
  const [x, y] = vectors[{ top: 'up', right: 'right', bottom: 'down', left: 'left' }[edge]];
  const slide = travel && styles.translate === 'none';
  return [
    { clipPath: 'inset(' + inset.join(' ') + round + ')', opacity: .38,
      ...(slide ? { translate: `${x * travel}px ${y * travel}px` } : {}) },
    { clipPath: 'inset(0' + round + ')', ...(slide ? { translate: '0px 0px' } : {}) },
  ];
}

export const recipes = [
  recipe('press-settle', 'Compress and release', 'Controls', 'Buttons, links and toggles', ['control'], 'Press', 'control', 140,
    'Compresses slightly on press and releases quickly.',
    [{ scale: '1', easing: 'ease-out' }, { scale: '.97', offset: .35, easing: easeOut }, { scale: '1' }],
    { easing: 'linear', relative: true }),
  recipe('like-pop', 'Pop and tilt', 'Controls', 'Icons and action controls', ['icon', 'control'], 'Activate', 'icon', 260,
    'Lifts a small icon with a tilt and a soft return.',
    [{ scale: '1', rotate: '0deg', easing: easeOut }, { scale: '1.15', rotate: '-6deg', offset: .38 },
      { scale: '.98', rotate: '1deg', offset: .74 }, { scale: '1', rotate: '0deg' }],
    { easing: 'linear', relative: true }),
  recipe('confirm-check', 'Scale in and settle', 'Feedback', 'Icons, status labels and alerts', ['icon', 'status'], 'Success', 'icon', 220,
    'Brings a confirmation into focus with a small overshoot.',
    [{ scale: '.92', opacity: .2, easing: easeOut }, { scale: '1.045', opacity: 1, offset: .58 }, {}],
    { easing: 'linear' }),
  recipe('status-crossfade', 'Fade in place', 'Feedback', 'Status labels, alerts and text', ['status', 'text'], 'State change', 'row', 180,
    'Fades the existing element in without moving it.',
    [{ opacity: .25 }, {}]),
  recipe('retry-nudge', 'Side-to-side nudge', 'Feedback', 'Fields, status labels and alerts', ['field', 'status'], 'Error', 'control', 200,
    'Briefly nudges the field that needs attention.',
    [{}, { translate: '-4px 0', offset: .22 }, { translate: '3px 0', offset: .49 },
      { translate: '-1px 0', offset: .74 }, {}], { easing: 'linear' }),
  recipe('new-row-settle', 'Slide up and fade in', 'Content', 'Rows and list items', ['row'], 'Insert', 'row', 220,
    'Glides up from just below its resting position as it fades in.',
    [{ translate: '0 7px', opacity: .18 }, {}]),
  recipe('detail-reveal', 'Slide down and fade in', 'Content', 'Detail sections and panels', ['panel'], 'Open', 'surface', 200,
    'Glides down from just above its resting position as it fades in.',
    [{ translate: '0 -5px', opacity: .25 }, {}]),
  recipe('popover-enter', 'Open from the anchor', 'Surfaces', 'Panels, menus and popovers', ['panel', 'overlay'], 'Open', 'surface', 170,
    'Scales and fades in from the menu’s attachment point.',
    [{ scale: '.94', opacity: 0 }, { scale: '1', opacity: 1 }],
    { anchored: true }),
  recipe('dialog-enter', 'Rise and scale in', 'Surfaces', 'Dialogs and alert dialogs', ['dialog'], 'Open', 'surface', 240,
    'Moves an existing dialog up from a small offset while scaling and fading in.',
    [{ translate: '0 6px', scale: '.965', opacity: 0 }, {}]),
  recipe('tab-indicator', 'Widen and fade in', 'Controls', 'Tabs and selection indicators', ['tab'], 'Selection', 'row', 170,
    'Widens a selection indicator in place as it fades in.',
    [{ scale: '.78 1', opacity: .25 }, {}]),
  recipe('reaction-burst', 'Reaction burst', 'Feedback', 'Icons and action controls', ['icon', 'control'], 'Activate', 'icon', 290,
    'Rebounds once while a few particles spread and fade.',
    [{ scale: '1' }, { scale: '.91', offset: .14, easing: easeOut },
      { scale: '1.13', offset: .44 }, { scale: '.985', offset: .72 }, { scale: '1' }],
    { easing: 'linear', relative: true, particles: true }),
  recipe('directional-shift', 'Directional nudge', 'Controls', 'Directional links, buttons and icons', ['control', 'icon'], 'Hover', 'link', 210,
    'Moves briefly toward a destination, then returns.', directionalFrames, {
      relative: true, easing: 'linear', skipReduced: true, requiresTransformableBox: true,
      presets: {
        subtle: { title: 'Subtle guide', direction: 'right', distance: 3 },
        pronounced: { title: 'Pronounced cue', direction: 'right', distance: 6 },
      },
      controls: { direction: { label: 'Direction', choices: directions }, distance: { label: 'Travel (px)', min: 1, max: 8 } },
    }),
  recipe('icon-turn', 'Turn to disclose', 'Controls', 'Chevron, caret and other state icons', ['icon'], 'State change', 'icon', 185,
    'Turns a state icon into its final orientation.', turnFrames, {
      relative: true, skipReduced: true, requiresTransformableBox: true,
      presets: {
        quarter: { title: 'Quarter turn', degrees: '90', direction: 'clockwise' },
        half: { title: 'Half turn', degrees: '180', direction: 'counterclockwise' },
      },
      controls: {
        degrees: { label: 'Angle', choices: { 45: '45°', 90: '90°', 180: '180°' } },
        direction: { label: 'Rotation', choices: { clockwise: 'Clockwise', counterclockwise: 'Counterclockwise' } },
      },
    }),
  recipe('edge-reveal', 'Reveal from an edge', 'Content', 'Panels, menus and rows', ['panel', 'overlay', 'row'], 'Open', 'surface', 230,
    'Uncovers the surface from one edge without resizing it.', revealFrames, {
      presets: {
        partial: { title: 'Partial reveal', edge: 'top', extent: 55, travel: 3 },
        full: { title: 'Full reveal', edge: 'top', extent: 100, travel: 6 },
      },
      controls: {
        edge: { label: 'Reveal edge', choices: { top: 'Top', right: 'Right', bottom: 'Bottom', left: 'Left' } },
        extent: { label: 'Initially hidden (%)', min: 20, max: 100 },
        travel: { label: 'Travel (px)', min: 0, max: 12 },
      },
    }),
  recipe('focus-halo', 'Focus halo', 'Controls', 'Fields, tabs and focusable controls', ['field', 'tab', 'control'], 'Focus', 'field', 220,
    'Adds a short expanding outline around the focused control.', null, {
      accent: 'halo', skipReduced: true, requiresFocus: true,
      presets: {
        compact: { title: 'Compact control', spread: 3, thickness: 1 },
        field: { title: 'Roomy field', spread: 6, thickness: 2 },
      },
      controls: {
        spread: { label: 'Spread (px)', min: 2, max: 10 }, thickness: { label: 'Thickness (px)', min: 1, max: 3 },
        color: { label: 'Halo color' },
      },
    }),
  recipe('ink-ripple', 'Ink ripple', 'Feedback', 'Pressable controls', ['control'], 'Press', 'control', 260,
    'Spreads a translucent wash inside the pressed control.', null, {
      accent: 'ripple', skipReduced: true,
      presets: {
        center: { title: 'Centered wash', origin: 'center', intensity: 18 },
        offset: { title: 'Offset accent', origin: 'start', intensity: 25 },
      },
      controls: {
        origin: { label: 'Origin', choices: { center: 'Center', start: 'Inline start', end: 'Inline end' } },
        intensity: { label: 'Peak opacity (%)', min: 8, max: 32 }, color: { label: 'Ripple color' },
      },
    }),
  recipe('particles', 'Particles', 'Feedback', 'Any visible element', ['any'], 'Custom', 'spark', 270,
    'Sends a small spray of shapes out from the element.', null, { particles: true }),
  recipe('reference-pattern', 'Reference patterns', 'References', 'Any visible element', ['any'], 'Custom', 'pattern', 220,
    'Choose an interaction for the agent to mock up.', null),
];

const kindSelectors = new Map([
    ['control', 'button,a[href],summary,input:is([type=button],[type=submit],[type=reset],[type=checkbox],[type=radio]),[role=button],[role=link],[role=switch],[role=checkbox],[role=radio],[role=menuitem],[role=menuitemcheckbox],[role=menuitemradio]'],
    ['icon', 'svg,img,[role=img],[data-icon]'],
    ['field', 'input,select,textarea,[role=textbox],[role=combobox],[role=slider],[contenteditable=true]'],
    ['status', 'output,[role=status],[role=alert],[aria-live]:not([aria-live=off])'],
    ['text', 'p,span,small,label,strong,em,h1,h2,h3,h4,h5,h6'],
    ['row', 'li,tr,[role=listitem],[role=row],[role=option]'],
    ['panel', 'section,article,aside,nav,fieldset,details,[role=region],[role=group],[role=tabpanel]'],
    ['overlay', '[popover],[role=menu],[role=listbox],[role=tooltip]'],
    ['dialog', 'dialog,[role=dialog],[role=alertdialog]'],
    ['tab', '[role=tab],[data-selection-indicator]'],
]);

const recipeSelector = ({ kinds }) => kinds.map((kind) => kindSelectors.get(kind)).join(',');

export function recipeMatchesElement(recipe, element) {
  if (recipe.kinds.includes('any')) return true;
  if (!element.matches(recipeSelector(recipe))) return false;
  if (recipe.requiresFocus) {
    return !element.matches(':disabled,[aria-disabled=true]') &&
      (element.tabIndex >= 0 || element.hasAttribute('tabindex') || element.isContentEditable);
  }
  if (recipe.requiresTransformableBox) {
    const { display } = element.ownerDocument.defaultView.getComputedStyle(element);
    return display !== 'contents' && (display !== 'inline' || element.matches('img,svg'));
  }
  return true;
}

export function recipesForElement(element) {
  return recipes.filter((recipe) => recipeMatchesElement(recipe, element));
}

export function elementsForRecipe(doc, recipe) {
  if (recipe.kinds.includes('any')) return null;
  const matches = doc.querySelectorAll(recipeSelector(recipe));
  if (recipe.requiresFocus || recipe.requiresTransformableBox) {
    return Array.from(matches).filter((element) => recipeMatchesElement(recipe, element));
  }
  return matches;
}

export function findRecipe(key) {
  return recipes.find(({ id, version }) => key === id + '@' + version);
}

const hasChoice = (choices, value) => typeof value === 'string' && Object.hasOwn(choices, value);
const isHexColor = (value) => typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value);

export function effectSettings(recipe, { preset = Object.keys(recipe.presets)[0], options } = {}) {
  const { title, ...defaults } = recipe.presets[preset];
  return { ...defaults, ...options };
}

function validateEffectSettings(recipe, { preset, options }, text) {
  if (preset !== undefined && !hasChoice(recipe.presets, preset)) return text('Choose a known preset for this effect.');
  if (options === undefined) return null;
  if (!options || typeof options !== 'object' || Array.isArray(options)) return text('Provide an object with effect options.');
  for (const [key, value] of Object.entries(options)) {
    if (!Object.hasOwn(recipe.controls, key)) return text('Unknown effect option: {0}.', key);
    const { label, choices, min, max } = recipe.controls[key];
    if (choices && !hasChoice(choices, value)) return text('Choose a known value for {0}.', text(label));
    if (min !== undefined && (!Number.isInteger(value) || value < min || value > max)) {
      return text('{0} must be a whole number between {1} and {2}.', text(label), min, max);
    }
    if (key === 'color' && !isHexColor(value)) return text('Use a six-digit hex color or leave it unset to inherit the element color.');
  }
  return null;
}

export function validateMotion(binding, text = (message, ...values) => message.replace(/\{(\d+)\}/g, (_, index) => values[index])) {
  const recipe = findRecipe(binding?.recipe);
  if (!recipe) return text('Choose a known, versioned motion recipe.');
  if (typeof binding.selector !== 'string' || !binding.selector.trim() || binding.selector.length > 512) {
    return text('Provide a CSS selector for exactly one preview element (up to 512 characters).');
  }
  if (!Number.isInteger(binding.duration) || binding.duration < 80 || binding.duration > 300) {
    return text('Use a duration between 80 and 300 ms.');
  }
  if (binding.variationOf !== undefined && (
    typeof binding.variationOf !== 'string' || !/^\d+-[0-9a-f]{6}\.json$/.test(binding.variationOf)
  )) return text('Provide a valid source suggestion ID.');
  if (binding.trigger !== undefined && !hasChoice(triggers, binding.trigger)) return text('Choose a known trigger.');
  if (binding.recipe === 'reference-pattern@1') {
    if (!hasChoice(motionReferences, binding.reference)) return text('Choose a reference pattern.');
  } else if (binding.reference !== undefined) return text('Reference patterns require the Reference patterns recipe.');
  if (recipe.particles) {
    if (binding.preset !== undefined && !hasChoice(particlePresets, binding.preset)) return text('Choose a known particle preset.');
    if (binding.options !== undefined) {
      const options = binding.options;
      if (!options || typeof options !== 'object' || Array.isArray(options) ||
          Object.keys(options).some((key) => !['shape', 'trajectory', 'count', 'distance', 'color'].includes(key))) {
        return text('Use only shape, trajectory, count, distance and color as particle options.');
      }
      const settings = particleSettings(binding);
      if (!hasChoice(particleShapes, settings.shape) || !hasChoice(particleTrajectories, settings.trajectory)) {
        return text('Choose a known particle shape and trajectory.');
      }
      if (!Number.isInteger(settings.count) || settings.count < 3 || settings.count > 16 ||
          !Number.isInteger(settings.distance) || settings.distance < 8 || settings.distance > 48) {
        return text('Use 3–16 particles and a spread of 8–48 px.');
      }
      if (settings.color !== undefined && !isHexColor(settings.color)) {
        return text('Use a six-digit hex color or leave it unset to inherit the element color.');
      }
    }
  } else if (recipe.controls) return validateEffectSettings(recipe, binding, text);
  else if (binding.preset !== undefined || binding.options !== undefined) return text('Choose a recipe that supports configurable settings.');
  return null;
}

export function particleSettings({ preset = 'soft-sparks', options }) {
  const { shape, trajectory, count, distance } = particlePresets[preset];
  return { shape, trajectory, count, distance, ...options };
}

function playParticles(target, recipe, binding) {
  const { shape, trajectory, count, distance, color } = particleSettings(binding);
  const reaction = recipe.id === 'reaction-burst';
  const doc = target.ownerDocument;
  const { width, height, top, left } = target.getBoundingClientRect();
  const container = binding.particleContainer;
  const layer = doc.createElement('span');
  layer.dataset.jiuParticles = '';
  layer.setAttribute('aria-hidden', 'true');
  Object.assign(layer.style, {
    position: container ? 'absolute' : 'fixed',
    top: container ? '50%' : top + height / 2 + 'px',
    left: container ? '50%' : left + width / 2 + 'px',
    width: '0', height: '0', pointerEvents: 'none', zIndex: '2147483647',
    color: color ?? doc.defaultView.getComputedStyle(target).color,
  });
  (container ?? doc.body).append(layer);
  const silhouettes = {
    spark: 'polygon(50% 0, 61% 38%, 100% 50%, 61% 62%, 50% 100%, 39% 62%, 0 50%, 39% 38%)',
    star: 'polygon(50% 0, 64% 33%, 100% 38%, 73% 63%, 81% 100%, 50% 80%, 19% 100%, 27% 63%, 0 38%, 36% 33%)',
    diamond: 'polygon(50% 0, 100% 50%, 50% 100%, 0 50%)',
  };
  const animations = Array.from({ length: count }, (_, index) => {
    const angle = Math.PI * 2 * (index + .28) / count;
    const radius = distance * (.62 + (index % 4) * .13);
    const x = Math.cos(angle) * radius;
    const y = trajectory === 'burst' ? Math.sin(angle) * radius :
      (trajectory === 'rise' ? -1 : 1) * radius * (.7 + Math.abs(Math.sin(angle)) * .3);
    const mote = doc.createElement('span');
    const size = (shape === 'dot' ? 3 : 5) + (index % 3);
    const delay = (reaction ? binding.duration * .12 : 0) + (index % 3) * 8;
    Object.assign(mote.style, {
      position: 'absolute', width: size + 'px', height: size + 'px', translate: '-50% -50%',
      boxSizing: 'border-box', background: shape === 'ring' ? 'transparent' : 'currentColor',
      border: shape === 'ring' ? '1px solid currentColor' : '',
      borderRadius: shape === 'dot' || shape === 'ring' ? '50%' : '',
      clipPath: silhouettes[shape] ?? '',
    });
    layer.append(mote);
    return mote.animate([
      { transform: 'scale(.5)', opacity: 0, easing: easeOut },
      { transform: `translate(${x * .65}px,${y * .65}px) rotate(15deg)`, opacity: 1, offset: .28, easing: easeOut },
      { transform: `translate(${x}px,${y}px) rotate(${30 + index * 7}deg) scale(.65)`, opacity: 0 },
    ], { id: 'jiu-particle-preview', duration: binding.duration - delay, delay, fill: 'backwards' });
  });
  if (reaction) {
    const ring = doc.createElement('span');
    const diameter = Math.max(12, Math.min(28, distance)) + 'px';
    Object.assign(ring.style, {
      position: 'absolute', width: diameter, height: diameter, translate: '-50% -50%',
      boxSizing: 'border-box', border: '1px solid currentColor', borderRadius: '50%',
    });
    layer.prepend(ring);
    animations.push(ring.animate([
      { scale: '.68', opacity: 0 },
      { scale: '1', opacity: .65, offset: .24 },
      { scale: '1.42', opacity: 0 },
    ], { id: 'jiu-particle-preview', duration: Math.round(binding.duration * .78),
      delay: Math.round(binding.duration * .12), fill: 'backwards', easing: 'linear' }));
  }
  void Promise.allSettled(animations.map((animation) => animation.finished)).then(() => layer.remove());
  return {
    cancel() {
      for (const animation of animations) animation.cancel();
      layer.remove();
    },
  };
}

function playAccent(target, recipe, binding) {
  const { duration, accentContainer } = binding;
  const { color, spread, thickness, origin, intensity } = effectSettings(recipe, binding);
  const doc = target.ownerDocument;
  const styles = doc.defaultView.getComputedStyle(target);
  const rect = target.getBoundingClientRect();
  const anchor = accentContainer?.getBoundingClientRect();
  const layer = doc.createElement('span');
  layer.dataset.jiuAccent = recipe.accent;
  layer.setAttribute('aria-hidden', 'true');
  Object.assign(layer.style, {
    position: accentContainer ? 'absolute' : 'fixed', display: 'block', boxSizing: 'border-box',
    left: rect.left - (anchor?.left ?? 0) + 'px', top: rect.top - (anchor?.top ?? 0) + 'px',
    width: rect.width + 'px', height: rect.height + 'px', pointerEvents: 'none', zIndex: '2147483647',
    color: color ?? styles.color, borderRadius: styles.borderRadius,
  });
  (accentContainer ?? doc.body).append(layer);

  let ink = layer;
  let frames;
  if (recipe.accent === 'halo') {
    // The actual focus indicator belongs to the control and stays visible after this pulse.
    const ring = (size) => '0 0 0 ' + size + 'px currentColor';
    frames = [
      { boxShadow: ring(thickness), opacity: .8, easing: easeOut },
      { boxShadow: ring(thickness + spread * .7), opacity: .37, offset: .58 },
      { boxShadow: ring(thickness + spread), opacity: 0 },
    ];
  } else {
    layer.style.overflow = 'hidden';
    const start = styles.direction === 'rtl' ? .76 : .24;
    const x = rect.width * ({ start, end: 1 - start, center: .5 })[origin];
    const y = rect.height * .5;
    const diameter = Math.hypot(Math.max(x, rect.width - x), y) * 2;
    ink = doc.createElement('span');
    Object.assign(ink.style, {
      position: 'absolute', width: diameter + 'px', height: diameter + 'px', left: x + 'px', top: y + 'px',
      translate: '-50% -50%', borderRadius: '50%', background: 'currentColor',
    });
    layer.append(ink);
    frames = [
      { scale: '.12', opacity: 0, easing: easeOut },
      { scale: '.68', opacity: intensity / 100, offset: .3, easing: easeOut },
      { scale: '1', opacity: 0 },
    ];
  }
  const animation = ink.animate(frames, { id: 'jiu-accent-preview', duration, easing: 'linear' });
  void animation.finished.then(() => layer.remove(), () => layer.remove());
  return { cancel() { animation.cancel(); layer.remove(); } };
}

export function playMotion(target, recipe, binding = {}) {
  const { duration = recipe.duration, reduced = false } = binding;
  if (reduced && recipe.skipReduced) return null;
  if (recipe.accent) return playAccent(target, recipe, { ...binding, duration });
  const particles = recipe.particles && !reduced ? playParticles(target, recipe, { ...binding, duration }) : null;
  if (!recipe.keyframes) return particles;
  const dynamic = typeof recipe.keyframes === 'function';
  let keyframes = dynamic
    ? (reduced ? [] : recipe.keyframes(effectSettings(recipe, binding), target.ownerDocument.defaultView.getComputedStyle(target)))
    : recipe.keyframes;
  if (reduced || recipe.anchored || keyframes.some((frame) => 'opacity' in frame)) {
    const styles = target.ownerDocument.defaultView.getComputedStyle(target);
    const opacity = Number(styles.opacity);
    if (reduced) keyframes = [{ opacity: .82 * opacity }, {}];
    else {
      keyframes = keyframes.map((frame) => 'opacity' in frame ? { ...frame, opacity: frame.opacity * opacity } : frame);
      if (recipe.anchored) {
        const edge = { top: 'center bottom', bottom: 'center top', left: 'right center', right: 'left center' };
        const origin = styles.getPropertyValue('--radix-popover-content-transform-origin').trim()
          || edge[target.getAttribute('data-side')] || styles.transformOrigin;
        keyframes = keyframes.map((frame) => ({ ...frame, transformOrigin: origin }));
      }
    }
  }
  const animation = target.animate(keyframes, {
    id: 'jiu-motion-preview', duration: reduced ? Math.min(duration, 100) : duration, easing: recipe.easing,
    composite: recipe.relative && !reduced ? 'add' : 'replace',
  });
  if (!particles) return animation;
  return { cancel() { animation.cancel(); particles.cancel(); } };
}

export function isLocalMock(preview) {
  return /^previews\//.test(preview?.path?.replaceAll('\\', '/') ?? '') && Boolean(preview.hash);
}
