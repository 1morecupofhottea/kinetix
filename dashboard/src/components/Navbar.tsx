import React from 'react';
import { ShieldCheck, Activity, DollarSign, Play, Key, Shuffle, Server, Users, BarChart3, Radio, Compass, History, LogOut, RefreshCw } from 'lucide-react';
import { ProxyMetrics } from '../types';
import { formatCurrency } from '../lib/designSystem';
import { SketchButton, SketchBadge } from './HandDrawnElements';

export type NavTab = 'keys' | 'routes' | 'providers' | 'accounts' | 'usage' | 'requests' | 'aliases' | 'audit';

export const TAB_ROUTES: Record<NavTab, string> = {
  keys: '/admin/keys',
  routes: '/admin/routes',
  providers: '/admin/providers',
  accounts: '/admin/accounts',
  usage: '/admin/usage',
  requests: '/admin/requests',
  aliases: '/admin/aliases',
  audit: '/admin/audit',
};

interface NavbarProps {
  activeTab: NavTab;
  onSelectTab: (tab: NavTab) => void;
  metrics: ProxyMetrics;
  onOpenTester: () => void;
  currentUser?: string;
  onLogout?: () => void;
  onRefresh?: () => void;
  isRefreshing?: boolean;
}

export const Navbar: React.FC<NavbarProps> = ({
  activeTab,
  onSelectTab,
  metrics,
  onOpenTester,
  currentUser,
  onLogout,
  onRefresh,
  isRefreshing,
}) => {
  const tabs: { id: NavTab; label: string; icon: React.ReactNode; badge?: string; path: string }[] = [
    { id: 'keys', label: 'Virtual Keys', icon: <Key className="w-5 h-5" />, path: TAB_ROUTES.keys },
    { id: 'routes', label: 'Routes & Fallback', icon: <Shuffle className="w-5 h-5" />, badge: 'Active', path: TAB_ROUTES.routes },
    { id: 'providers', label: 'Upstream Providers', icon: <Server className="w-5 h-5" />, path: TAB_ROUTES.providers },
    { id: 'accounts', label: 'Accounts & Pools', icon: <Users className="w-5 h-5" />, path: TAB_ROUTES.accounts },
    { id: 'usage', label: 'Usage & Spend', icon: <BarChart3 className="w-5 h-5" />, path: TAB_ROUTES.usage },
    { id: 'requests', label: 'Request Inspector', icon: <Radio className="w-5 h-5" />, badge: 'Live', path: TAB_ROUTES.requests },
    { id: 'aliases', label: 'Model Aliases', icon: <Compass className="w-5 h-5" />, path: TAB_ROUTES.aliases },
    { id: 'audit', label: 'Audit Log', icon: <History className="w-5 h-5" />, path: TAB_ROUTES.audit },
  ];

  return (
    <header className="w-full bg-[#fdfbf7] border-b-2 border-[#2d2d2d] pb-2 pt-3 px-4 md:px-8 relative">
      {/* Top row: Brand & Status Widgets */}
      <div className="max-w-7xl mx-auto flex flex-col md:flex-row items-start md:items-center justify-between gap-4 pb-3">
        <div className="flex items-center gap-3">
          {/* Hand-drawn Logo */}
          <div
            className="w-11 h-11 bg-[#ff4d4d] text-white flex items-center justify-center font-heading font-bold text-2xl border-2 border-[#2d2d2d] sketch-shadow -rotate-2 select-none"
            style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
          >
            K
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-3xl font-heading font-bold tracking-tight text-[#2d2d2d]">
                Kinetix
              </h1>
              <SketchBadge variant="yellow" rotation="1deg" className="text-xs font-heading">
                v0.1 Proxy
              </SketchBadge>
            </div>
            <p className="text-sm text-[#2d2d2d]/80 font-body -mt-1">
              Multi-Protocol LLM Proxy for <span className="font-bold underline decoration-wavy decoration-[#ff4d4d]">Pi</span> & Team Tools
            </p>
          </div>
        </div>

        {/* Live Proxy Indicators & Test Button */}
        <div className="flex flex-wrap items-center gap-3 w-full md:w-auto justify-end">
          {/* Cloudflare Tunnel Status */}
          <div
            className="flex items-center gap-2 bg-white px-3 py-1.5 border-2 border-[#2d2d2d] sketch-shadow-sm text-sm"
            style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
          >
            <div className="w-2.5 h-2.5 rounded-full bg-[#2e7d32] animate-pulse border border-[#2d2d2d]" />
            <span className="font-body text-[#2d2d2d]">
              <ShieldCheck className="w-4 h-4 inline-block mr-1 text-[#2d5da1]" />
              Tunnel: <strong className="font-heading">Online</strong> (Cloudflare)
            </span>
          </div>

          {/* Active Streams */}
          <div
            className="flex items-center gap-1.5 bg-white px-3 py-1.5 border-2 border-[#2d2d2d] sketch-shadow-sm text-sm"
            style={{ borderRadius: '255px 25px 225px 25px / 25px 225px 25px 255px' }}
          >
            <Activity className="w-4 h-4 text-[#ff4d4d]" />
            <span className="font-body">
              Active Streams: <strong className="font-heading text-base">{metrics.activeStreams}</strong>
            </span>
          </div>

          {/* Daily Spend */}
          <div
            className="flex items-center gap-1.5 bg-[#fff9c4] px-3 py-1.5 border-2 border-[#2d2d2d] sketch-shadow-sm text-sm"
            style={{ borderRadius: '20px 300px 20px 280px / 280px 20px 300px 20px' }}
          >
            <DollarSign className="w-4 h-4 text-[#2d5da1]" />
            <span className="font-body">
              Spend: <strong className="font-heading text-base">{formatCurrency(metrics.totalSpendUsd)}</strong>
            </span>
          </div>

          {/* Refresh */}
          {onRefresh && (
            <button
              onClick={onRefresh}
              disabled={isRefreshing}
              className="flex items-center gap-1.5 bg-white px-2.5 py-1.5 border-2 border-[#2d2d2d] sketch-shadow-sm text-sm font-heading font-bold cursor-pointer hover:bg-[#e5e0d8] disabled:opacity-60"
              style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
              title="Reload all data from the admin API"
            >
              <RefreshCw className={`w-4 h-4 ${isRefreshing ? 'animate-spin' : ''}`} />
              <span className="hidden sm:inline">Refresh</span>
            </button>
          )}

          {/* Test Proxy Simulator Trigger */}
          <SketchButton
            id="btn-test-proxy"
            variant="danger"
            size="sm"
            onClick={onOpenTester}
            className="gap-1.5 font-heading font-bold"
          >
            <Play className="w-4 h-4 fill-white" />
            Live Proxy Test
          </SketchButton>

          {/* User Session & Logout */}
          {currentUser && (
            <div className="flex items-center gap-1.5 pl-1 border-l-2 border-[#2d2d2d]/30">
              <div
                className="hidden sm:flex items-center gap-1.5 bg-white px-2.5 py-1 border-2 border-[#2d2d2d] sketch-shadow-sm text-xs font-mono"
                style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                title={`Active Gateway Session: ${currentUser}`}
              >
                <div className="w-2 h-2 rounded-full bg-[#2e7d32]" />
                <span className="text-[#2d2d2d] truncate max-w-[110px] font-bold">
                  {currentUser}
                </span>
              </div>

              {onLogout && (
                <button
                  id="btn-logout"
                  onClick={onLogout}
                  className="px-2.5 py-1.5 bg-white hover:bg-[#ffebee] text-[#2d2d2d] hover:text-[#ff4d4d] border-2 border-[#2d2d2d] rounded sketch-shadow-sm cursor-pointer transition-colors flex items-center gap-1 text-xs font-heading font-bold"
                  title="Sign Out / Lock Gateway"
                >
                  <LogOut className="w-3.5 h-3.5" />
                  <span className="hidden md:inline">Sign Out</span>
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Tabs navigation row styled like hand-drawn file folder tabs */}
      <div className="max-w-7xl mx-auto overflow-x-auto pt-2 pb-1 scrollbar-none">
        <nav className="flex items-center gap-2 min-w-max">
          {tabs.map((tab, idx) => {
            const isActive = activeTab === tab.id;
            const tilt = idx % 2 === 0 ? '-rotate-0.5' : 'rotate-0.5';

            return (
              <a
                key={tab.id}
                id={`tab-${tab.id}`}
                href={tab.path}
                onClick={(e) => {
                  e.preventDefault();
                  onSelectTab(tab.id);
                }}
                className={`group relative flex items-center gap-2 px-3.5 py-1.5 font-heading text-base md:text-lg border-2 border-[#2d2d2d] transition-all select-none no-underline cursor-pointer ${tilt} ${
                  isActive
                    ? 'bg-white text-[#2d2d2d] border-b-0 -translate-y-1 shadow-[3px_3px_0px_0px_#2d2d2d] font-bold'
                    : 'bg-[#e5e0d8]/80 text-[#2d2d2d]/80 hover:bg-white hover:text-[#2d2d2d] shadow-[2px_2px_0px_0px_#2d2d2d]'
                }`}
                style={{
                  borderRadius: '16px 16px 0 0',
                }}
              >
                <span className={isActive ? 'text-[#ff4d4d]' : 'text-[#2d2d2d]/70'}>{tab.icon}</span>
                <span>{tab.label}</span>
                {tab.badge && (
                  <span
                    className={`text-xs px-1.5 py-0.2 rounded-full border border-[#2d2d2d] ${
                      tab.badge === 'Live' ? 'bg-[#ff4d4d] text-white animate-pulse' : 'bg-[#fff9c4] text-[#2d2d2d]'
                    }`}
                  >
                    {tab.badge}
                  </span>
                )}
                {isActive && (
                  <div className="absolute -bottom-1.5 left-0 right-0 h-2 bg-white z-20 border-l-2 border-r-2 border-white" />
                )}
              </a>
            );
          })}
        </nav>
      </div>
    </header>
  );
};
