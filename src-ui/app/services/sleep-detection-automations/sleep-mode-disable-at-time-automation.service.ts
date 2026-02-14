import { Injectable } from '@angular/core';
import { AutomationConfigService } from '../automation-config.service';
import {
  AUTOMATION_CONFIGS_DEFAULT,
  SleepModeDisableAtTimeAutomationConfig,
} from '../../models/automations';

import { map } from 'rxjs';
import { SleepService } from '../sleep.service';
import { time_to_wait } from 'src-ui/app/utils/time';

@Injectable({
  providedIn: 'root',
})
export class SleepModeDisableAtTimeAutomationService {
  private config: SleepModeDisableAtTimeAutomationConfig = structuredClone(
    AUTOMATION_CONFIGS_DEFAULT.SLEEP_MODE_DISABLE_AT_TIME
  );
  private timeout: NodeJS.Timeout | null = null;
  constructor(
    private automationConfig: AutomationConfigService,
    private sleep: SleepService
  ) {}

  async init() {
    this.automationConfig.configs
      .pipe(map((configs) => configs.SLEEP_MODE_DISABLE_AT_TIME))
      .subscribe((config) => {
        if (!config.enabled) {
          if (this.timeout){
            clearTimeout(this.timeout);
          }
        }
        if (!config || !config.time) {
          console.debug('SleepModeDisableAtTimeAutomationService config is null!');
          return;
        }
        if (config.enabled && this.config != config) {
          if (!config.enabled && this.timeout) {
            clearTimeout(this.timeout);
            return;
          }
          const duration = time_to_wait(config.time);
          this.config = config;

          console.log('firing SleepModeDisableAtTimeAutomationService in:' + duration + 'ms');
          this.timeout = setTimeout(() => this.disable(), duration);
        }
        this.config = config;
      });
  }

  async disable() {
    if (!this.config.enabled) {
      return;
    }
    this.sleep.disableSleepMode({
      type: 'AUTOMATION',
      automation: 'SLEEP_MODE_DISABLE_AT_TIME',
    });
    this.timeout = setTimeout(() => this.disable(), 24 * 3600 * 1000);
  }
}
