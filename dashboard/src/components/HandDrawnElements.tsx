import React from 'react';
import { DESIGN_TOKENS } from '../lib/designSystem';

interface TapeProps {
  className?: string;
  rotation?: string;
}

export const TapeStrip: React.FC<TapeProps> = ({ className = '', rotation = '-1.5deg' }) => (
  <div
    className={`absolute pointer-events-none z-10 ${className}`}
    style={{
      transform: `translateX(-50%) rotate(${rotation})`,
      width: '84px',
      height: '22px',
      backgroundColor: 'rgba(229, 224, 216, 0.88)',
      borderLeft: '2px dashed rgba(45, 45, 45, 0.35)',
      borderRight: '2px dashed rgba(45, 45, 45, 0.35)',
      boxShadow: '0 1px 2px rgba(0,0,0,0.08)',
    }}
  />
);

interface ThumbtackProps {
  color?: 'red' | 'blue';
  className?: string;
}

export const Thumbtack: React.FC<ThumbtackProps> = ({ color = 'red', className = '' }) => (
  <div
    className={`absolute pointer-events-none z-10 w-4 h-4 rounded-full border-2 border-[var(--ink)] ${className}`}
    style={{
      background:
        color === 'red'
          ? 'radial-gradient(circle at 35% 35%, #ff7676, var(--marker-red) 65%, #9a1a1a)'
          : 'radial-gradient(circle at 35% 35%, #76a5ff, var(--pen-blue) 65%, #153463)',
      boxShadow: '2px 2px 0px 0px rgba(45, 45, 45, 0.6)',
    }}
  />
);

interface WobblyCardProps {
  children: React.ReactNode;
  className?: string;
  decoration?: 'tape' | 'tack' | 'tack-blue' | 'none';
  variant?: 'white' | 'postit' | 'muted';
  rotation?: string;
  id?: string;
}

export const WobblyCard: React.FC<WobblyCardProps> = ({
  children,
  className = '',
  decoration = 'none',
  variant = 'white',
  rotation = '0deg',
  id,
}) => {
  const bgStyle =
    variant === 'postit'
      ? 'bg-[var(--postit)] text-[var(--ink)]'
      : variant === 'muted'
      ? 'bg-[var(--erased-soft)] text-[var(--ink)]'
      : 'bg-[var(--surface)] text-[var(--ink)]';

  return (
    <div
      id={id}
      className={`relative border-2 border-[var(--ink)] p-6 box-border sketch-shadow transition-transform duration-150 ${bgStyle} ${className}`}
      style={{
        borderRadius: DESIGN_TOKENS.radii.wobbly,
        transform: `rotate(${rotation})`,
      }}
    >
      {decoration === 'tape' && <TapeStrip className="top-[-10px] left-1/2" />}
      {decoration === 'tack' && <Thumbtack color="red" className="top-[-8px] left-1/2 -translate-x-1/2" />}
      {decoration === 'tack-blue' && <Thumbtack color="blue" className="top-[-8px] left-1/2 -translate-x-1/2" />}
      {children}
    </div>
  );
};

interface SketchButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
  size?: 'sm' | 'md' | 'lg';
  id?: string;
}

export const SketchButton: React.FC<SketchButtonProps> = ({
  children,
  variant = 'primary',
  size = 'md',
  className = '',
  id,
  ...props
}) => {
  const sizeStyles =
    size === 'sm'
      ? 'px-3 py-1 text-base'
      : size === 'lg'
      ? 'px-6 py-2.5 text-xl font-bold'
      : 'px-4 py-2 text-lg';

  let colorStyles = 'bg-[var(--surface)] text-[var(--ink)] border-2 border-[var(--ink)] hover:bg-[var(--marker-red)] hover:text-[var(--surface)]';
  if (variant === 'secondary') {
    colorStyles = 'bg-[var(--erased)] text-[var(--ink)] border-2 border-[var(--ink)] hover:bg-[var(--pen-blue)] hover:text-[var(--surface)]';
  } else if (variant === 'danger') {
    colorStyles = 'bg-[var(--marker-red)] text-[var(--surface)] border-2 border-[var(--ink)] hover:brightness-90';
  } else if (variant === 'ghost') {
    colorStyles = 'bg-transparent text-[var(--ink)] border-2 border-dashed border-[var(--ink)] hover:bg-[var(--erased)]';
  }

  return (
    <button
      id={id}
      className={`sketch-btn select-none font-body active:translate-x-1 active:translate-y-1 ${colorStyles} ${sizeStyles} ${className}`}
      style={{
        borderRadius: DESIGN_TOKENS.radii.wobblyBtn,
      }}
      {...props}
    >
      {children}
    </button>
  );
};

interface SketchBadgeProps {
  children: React.ReactNode;
  variant?: 'default' | 'red' | 'blue' | 'green' | 'yellow';
  className?: string;
  rotation?: string;
}

export const SketchBadge: React.FC<SketchBadgeProps> = ({
  children,
  variant = 'default',
  className = '',
  rotation = '0deg',
}) => {
  let colors = 'bg-[var(--surface)] text-[var(--ink)] border-[var(--ink)]';
  if (variant === 'red') colors = 'bg-[var(--tint-red)] text-[var(--danger-text)] border-[var(--marker-red)]';
  if (variant === 'blue') colors = 'bg-[var(--tint-blue)] text-[var(--info-text)] border-[var(--pen-blue)]';
  if (variant === 'green') colors = 'bg-[var(--tint-green)] text-[var(--success-text)] border-[var(--pen-green)]';
  if (variant === 'yellow') colors = 'bg-[var(--postit)] text-[var(--warn-text)] border-[var(--postit-border)]';

  return (
    <span
      className={`inline-flex items-center gap-1 px-2.5 py-0.5 text-sm font-medium border-2 sketch-shadow-sm whitespace-nowrap select-none ${colors} ${className}`}
      style={{
        borderRadius: DESIGN_TOKENS.radii.wobblyBadge,
        transform: `rotate(${rotation})`,
      }}
    >
      {children}
    </span>
  );
};

export const SquiggleDivider: React.FC<{ className?: string }> = ({ className = '' }) => (
  <div className={`w-full overflow-hidden py-3 select-none pointer-events-none opacity-70 ${className}`}>
    <svg viewBox="0 0 1200 16" className="w-full h-4 stroke-[var(--ink)] fill-none stroke-[2.5]" preserveAspectRatio="none">
      <path d="M0,8 Q30,1 60,8 T120,8 T180,8 T240,8 T300,8 T360,8 T420,8 T480,8 T540,8 T600,8 T660,8 T720,8 T780,8 T840,8 T900,8 T960,8 T1020,8 T1080,8 T1140,8 T1200,8" />
    </svg>
  </div>
);
