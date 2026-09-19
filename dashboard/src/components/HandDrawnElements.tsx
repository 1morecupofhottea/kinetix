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
    className={`absolute pointer-events-none z-10 w-4 h-4 rounded-full border-2 border-[#2d2d2d] ${className}`}
    style={{
      background:
        color === 'red'
          ? 'radial-gradient(circle at 35% 35%, #ff7676, #ff4d4d 65%, #9a1a1a)'
          : 'radial-gradient(circle at 35% 35%, #76a5ff, #2d5da1 65%, #153463)',
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
      ? 'bg-[#fff9c4] text-[#2d2d2d]'
      : variant === 'muted'
      ? 'bg-[#f4efe8] text-[#2d2d2d]'
      : 'bg-white text-[#2d2d2d]';

  return (
    <div
      id={id}
      className={`relative border-2 border-[#2d2d2d] p-6 box-border sketch-shadow transition-transform duration-150 ${bgStyle} ${className}`}
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

  let colorStyles = 'bg-white text-[#2d2d2d] border-2 border-[#2d2d2d] hover:bg-[#ff4d4d] hover:text-white';
  if (variant === 'secondary') {
    colorStyles = 'bg-[#e5e0d8] text-[#2d2d2d] border-2 border-[#2d2d2d] hover:bg-[#2d5da1] hover:text-white';
  } else if (variant === 'danger') {
    colorStyles = 'bg-[#ff4d4d] text-white border-2 border-[#2d2d2d] hover:bg-[#d32f2f]';
  } else if (variant === 'ghost') {
    colorStyles = 'bg-transparent text-[#2d2d2d] border-2 border-dashed border-[#2d2d2d] hover:bg-[#e5e0d8]';
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
  let colors = 'bg-white text-[#2d2d2d] border-[#2d2d2d]';
  if (variant === 'red') colors = 'bg-[#ffebee] text-[#b71c1c] border-[#ff4d4d]';
  if (variant === 'blue') colors = 'bg-[#e8f0fe] text-[#1a3d7c] border-[#2d5da1]';
  if (variant === 'green') colors = 'bg-[#e8f5e9] text-[#1b5e20] border-[#2e7d32]';
  if (variant === 'yellow') colors = 'bg-[#fff9c4] text-[#826b00] border-[#ecd76e]';

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
    <svg viewBox="0 0 1200 16" className="w-full h-4 stroke-[#2d2d2d] fill-none stroke-[2.5]" preserveAspectRatio="none">
      <path d="M0,8 Q30,1 60,8 T120,8 T180,8 T240,8 T300,8 T360,8 T420,8 T480,8 T540,8 T600,8 T660,8 T720,8 T780,8 T840,8 T900,8 T960,8 T1020,8 T1080,8 T1140,8 T1200,8" />
    </svg>
  </div>
);
